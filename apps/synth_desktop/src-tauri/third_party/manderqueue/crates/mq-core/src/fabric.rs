use std::sync::Arc;

use crate::error::{Error, Result};
use crate::memory::MemoryStore;
use crate::store::Store;
use crate::types::*;
use crate::wake::{NoopWake, Wake};

/// Fabric API over any [`Store`] (memory or Postgres).
#[derive(Clone)]
pub struct Fabric {
    store: Arc<dyn Store>,
    wake: Arc<dyn Wake>,
}

impl Default for Fabric {
    fn default() -> Self {
        Self::memory()
    }
}

impl Fabric {
    pub fn memory() -> Self {
        Self {
            store: Arc::new(MemoryStore::default()),
            wake: Arc::new(NoopWake),
        }
    }

    pub fn from_store(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            wake: Arc::new(NoopWake),
        }
    }

    pub fn with_wake(mut self, wake: Arc<dyn Wake>) -> Self {
        self.wake = wake;
        self
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

    pub async fn publish(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        req: PublishMessage,
    ) -> Result<Message> {
        let _thread = self.load_workspace_thread(actor, thread_id).await?;
        self.require_cap(actor, thread_id, Cap::Publish).await?;
        if req.body.is_empty() && req.kind != MessageKind::Notice {
            return Err(Error::Invalid("body_required"));
        }

        let members = self.store.list_participants(thread_id).await?;
        let recipients: Vec<Principal> = if req.recipients.is_empty() {
            members
                .iter()
                .filter(|p| {
                    p.caps.contains(&Cap::Read)
                        && p.principal != *actor
                        && p.principal.org_id == actor.org_id
                })
                .map(|p| p.principal.clone())
                .collect()
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
