//! Write buffer → batch flush to durable store (reduces PG load).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::grants::*;
use crate::store::Store;
use crate::types::*;
use chrono::DateTime;
use uuid::Uuid;

/// Optional side-channel (e.g. Redis list) for durable staging before/alongside flush.
#[async_trait]
pub trait PublishMirror: Send + Sync {
    async fn mirror(&self, item: &BufferedPublish);
}

/// One hot-path publish staged for durable flush.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferedPublish {
    pub org_id: String,
    pub message: Message,
    pub recipients: Vec<Principal>,
}

/// Counters for measuring durable write amplification.
#[derive(Debug, Default)]
pub struct StoreCounters {
    pub append_calls: AtomicU64,
    pub enqueue_calls: AtomicU64,
    pub flush_batch_calls: AtomicU64,
    pub flush_batch_items: AtomicU64,
}

impl StoreCounters {
    pub fn snapshot(&self) -> StoreCounterSnapshot {
        StoreCounterSnapshot {
            append_calls: self.append_calls.load(Ordering::Relaxed),
            enqueue_calls: self.enqueue_calls.load(Ordering::Relaxed),
            flush_batch_calls: self.flush_batch_calls.load(Ordering::Relaxed),
            flush_batch_items: self.flush_batch_items.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreCounterSnapshot {
    pub append_calls: u64,
    pub enqueue_calls: u64,
    pub flush_batch_calls: u64,
    pub flush_batch_items: u64,
}

/// Wraps a [`Store`] and counts hot-path write ops.
pub struct MeteredStore {
    inner: Arc<dyn Store>,
    pub counters: Arc<StoreCounters>,
}

impl MeteredStore {
    pub fn new(inner: Arc<dyn Store>) -> Self {
        Self {
            inner,
            counters: Arc::new(StoreCounters::default()),
        }
    }

    pub fn with_counters(inner: Arc<dyn Store>, counters: Arc<StoreCounters>) -> Self {
        Self { inner, counters }
    }
}

#[async_trait]
impl Store for MeteredStore {
    async fn insert_thread(&self, creator: &Principal, req: CreateThread) -> Result<Thread> {
        self.inner.insert_thread(creator, req).await
    }

    async fn find_thread_by_idempotency(
        &self,
        org_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<Thread>> {
        self.inner
            .find_thread_by_idempotency(org_id, idempotency_key)
            .await
    }

    async fn get_thread(&self, thread_id: ThreadId) -> Result<Option<Thread>> {
        self.inner.get_thread(thread_id).await
    }

    async fn read_scoped(
        &self, actor: &Principal, thread_id: ThreadId, generation: u64,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Vec<Message>)> {
        self.inner.read_scoped(actor, thread_id, generation, after_seq, limit).await
    }

    async fn enroll(&self, owner: &Principal, req: EnrollDevice, now: DateTime<Utc>) -> Result<Enrollment> {
        self.inner.enroll(owner, req, now).await
    }
    async fn get_enrollment(&self, enrollment_id: Uuid) -> Result<Option<Enrollment>> {
        self.inner.get_enrollment(enrollment_id).await
    }
    async fn list_enrollments(&self, owner: &Principal) -> Result<Vec<Enrollment>> {
        self.inner.list_enrollments(owner).await
    }
    async fn revoke_enrollment(&self, owner: &Principal, enrollment_id: Uuid, now: DateTime<Utc>) -> Result<Enrollment> {
        self.inner.revoke_enrollment(owner, enrollment_id, now).await
    }
    async fn create_grant(&self, actor: &Principal, req: CreateGrant, now: DateTime<Utc>) -> Result<Grant> {
        self.inner.create_grant(actor, req, now).await
    }
    async fn get_grant(&self, grant_id: Uuid, now: DateTime<Utc>) -> Result<Option<(Grant, Enrollment)>> {
        self.inner.get_grant(grant_id, now).await
    }
    async fn list_grants(&self, org_id: &str, filter: &GrantFilter, now: DateTime<Utc>) -> Result<Vec<(Grant, Enrollment)>> {
        self.inner.list_grants(org_id, filter, now).await
    }
    async fn mutate_grant(&self, actor: &Principal, grant_id: Uuid, mutation: GrantMutation, now: DateTime<Utc>) -> Result<Grant> {
        self.inner.mutate_grant(actor, grant_id, mutation, now).await
    }
    async fn grant_issuance(&self, actor: &Principal, grant_id: Uuid, req: GrantIssuanceRequest, now: DateTime<Utc>) -> Result<Grant> {
        self.inner.grant_issuance(actor, grant_id, req, now).await
    }
    async fn read_granted(
        &self, actor: &Principal, thread_id: ThreadId, fence: &GrantFence, after_seq: u64, limit: usize,
    ) -> Result<(Thread, Grant, Vec<Message>)> {
        self.inner.read_granted(actor, thread_id, fence, after_seq, limit).await
    }
    async fn delivery_grant(
        &self, thread_id: ThreadId, recipient: &Principal, message_seq: u64, now: DateTime<Utc>,
    ) -> Result<DeliveryGrant> {
        self.inner.delivery_grant(thread_id, recipient, message_seq, now).await
    }

    async fn list_threads(
        &self,
        org_id: &str,
        scope: Option<&ScopeBinding>,
    ) -> Result<Vec<Thread>> {
        self.inner.list_threads(org_id, scope).await
    }

    async fn has_cap(&self, thread_id: ThreadId, principal: &Principal, cap: Cap) -> Result<bool> {
        self.inner.has_cap(thread_id, principal, cap).await
    }

    async fn add_participant(&self, thread_id: ThreadId, participant: Participant) -> Result<()> {
        self.inner.add_participant(thread_id, participant).await
    }

    async fn set_participant_role(
        &self,
        thread_id: ThreadId,
        principal: &Principal,
        role: Role,
    ) -> Result<()> {
        self.inner
            .set_participant_role(thread_id, principal, role)
            .await
    }

    async fn mutate_participant(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        target: Participant,
        create: bool,
    ) -> Result<()> {
        self.inner
            .mutate_participant(actor, thread_id, target, create)
            .await
    }

    async fn append_message(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
    ) -> Result<(Message, bool)> {
        self.counters.append_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.append_message(thread_id, sender, req).await
    }

    async fn append_with_delivery(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
        recipients: &[Principal],
    ) -> Result<(Message, bool)> {
        self.counters.append_calls.fetch_add(1, Ordering::Relaxed);
        self.inner
            .append_with_delivery(thread_id, sender, req, recipients)
            .await
    }

    async fn read_messages(
        &self,
        thread_id: ThreadId,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        self.inner.read_messages(thread_id, after_seq, limit).await
    }

    async fn list_participants(&self, thread_id: ThreadId) -> Result<Vec<Participant>> {
        self.inner.list_participants(thread_id).await
    }

    async fn get_message(&self, message_id: MessageId) -> Result<Option<Message>> {
        self.inner.get_message(message_id).await
    }

    async fn enqueue_delivery_jobs(
        &self,
        message: &Message,
        recipients: &[Principal],
    ) -> Result<Vec<DeliveryJob>> {
        self.counters.enqueue_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.enqueue_delivery_jobs(message, recipients).await
    }

    async fn claim_delivery_jobs(&self, limit: usize) -> Result<Vec<DeliveryJob>> {
        self.inner.claim_delivery_jobs(limit).await
    }

    async fn settle_delivery_job(
        &self,
        job_id: DeliveryJobId,
        expected_attempt: u32,
        status: DeliveryStatus,
    ) -> Result<()> {
        self.inner
            .settle_delivery_job(job_id, expected_attempt, status)
            .await
    }

    async fn flush_write_batch(&self, batch: &[BufferedPublish]) -> Result<()> {
        self.counters
            .flush_batch_calls
            .fetch_add(1, Ordering::Relaxed);
        self.counters
            .flush_batch_items
            .fetch_add(batch.len() as u64, Ordering::Relaxed);
        self.inner.flush_write_batch(batch).await
    }
}

#[derive(Default)]
struct PendingState {
    queue: VecDeque<BufferedPublish>,
    /// Index into `queue` for attaching recipients after Fabric.enqueue.
    by_msg: HashMap<MessageId, usize>,
    idempotency: HashMap<(String, String), Message>,
    /// Next seq to assign per thread (after durable + pending).
    next_seq: HashMap<ThreadId, u64>,
}

impl PendingState {
    fn reindex(&mut self) {
        self.by_msg.clear();
        for (i, item) in self.queue.iter().enumerate() {
            self.by_msg.insert(item.message.message_id, i);
        }
    }
}

/// Stages `append_message` / `enqueue_delivery_jobs` in memory; [`Self::flush`]
/// writes a batch to the durable store in one `flush_write_batch` call.
///
/// Metadata (threads, membership) always passes through. Reads merge pending
/// buffer with durable so clients see read-your-writes before flush.
pub struct BatchingStore {
    durable: Arc<dyn Store>,
    pending: Mutex<PendingState>,
    mirror: Option<Arc<dyn PublishMirror>>,
}

impl BatchingStore {
    pub fn new(durable: Arc<dyn Store>) -> Self {
        Self {
            durable,
            pending: Mutex::new(PendingState::default()),
            mirror: None,
        }
    }

    pub fn with_mirror(mut self, mirror: Arc<dyn PublishMirror>) -> Self {
        self.mirror = Some(mirror);
        self
    }

    pub fn durable(&self) -> Arc<dyn Store> {
        self.durable.clone()
    }

    pub async fn pending_len(&self) -> usize {
        self.pending.lock().await.queue.len()
    }

    async fn mirror_item(&self, item: &BufferedPublish) {
        if let Some(m) = &self.mirror {
            m.mirror(item).await;
        }
    }

    /// Drain up to `max` buffered publishes into the durable store (one batch call).
    pub async fn flush(&self, max: usize) -> Result<usize> {
        let batch = {
            let mut g = self.pending.lock().await;
            let n = max.min(g.queue.len());
            if n == 0 {
                return Ok(0);
            }
            let batch: Vec<_> = g.queue.drain(..n).collect();
            g.reindex();
            batch
        };
        let n = batch.len();
        self.durable.flush_write_batch(&batch).await?;
        Ok(n)
    }

    pub async fn flush_all(&self) -> Result<usize> {
        self.flush(usize::MAX).await
    }
}

#[async_trait]
impl Store for BatchingStore {
    async fn insert_thread(&self, creator: &Principal, req: CreateThread) -> Result<Thread> {
        self.durable.insert_thread(creator, req).await
    }

    async fn find_thread_by_idempotency(
        &self,
        org_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<Thread>> {
        self.durable
            .find_thread_by_idempotency(org_id, idempotency_key)
            .await
    }

    async fn get_thread(&self, thread_id: ThreadId) -> Result<Option<Thread>> {
        self.durable.get_thread(thread_id).await
    }

    async fn read_scoped(
        &self, actor: &Principal, thread_id: ThreadId, generation: u64,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Vec<Message>)> {
        // Scoped writes already bypass the volatile buffer. Never merge an
        // independently authorized buffer snapshot into a scoped read.
        self.durable.read_scoped(actor, thread_id, generation, after_seq, limit).await
    }

    // Grant authority never passes through the volatile buffer.
    async fn enroll(&self, owner: &Principal, req: EnrollDevice, now: DateTime<Utc>) -> Result<Enrollment> {
        self.durable.enroll(owner, req, now).await
    }
    async fn get_enrollment(&self, enrollment_id: Uuid) -> Result<Option<Enrollment>> {
        self.durable.get_enrollment(enrollment_id).await
    }
    async fn list_enrollments(&self, owner: &Principal) -> Result<Vec<Enrollment>> {
        self.durable.list_enrollments(owner).await
    }
    async fn revoke_enrollment(&self, owner: &Principal, enrollment_id: Uuid, now: DateTime<Utc>) -> Result<Enrollment> {
        self.durable.revoke_enrollment(owner, enrollment_id, now).await
    }
    async fn create_grant(&self, actor: &Principal, req: CreateGrant, now: DateTime<Utc>) -> Result<Grant> {
        self.durable.create_grant(actor, req, now).await
    }
    async fn get_grant(&self, grant_id: Uuid, now: DateTime<Utc>) -> Result<Option<(Grant, Enrollment)>> {
        self.durable.get_grant(grant_id, now).await
    }
    async fn list_grants(&self, org_id: &str, filter: &GrantFilter, now: DateTime<Utc>) -> Result<Vec<(Grant, Enrollment)>> {
        self.durable.list_grants(org_id, filter, now).await
    }
    async fn mutate_grant(&self, actor: &Principal, grant_id: Uuid, mutation: GrantMutation, now: DateTime<Utc>) -> Result<Grant> {
        self.durable.mutate_grant(actor, grant_id, mutation, now).await
    }
    async fn grant_issuance(&self, actor: &Principal, grant_id: Uuid, req: GrantIssuanceRequest, now: DateTime<Utc>) -> Result<Grant> {
        self.durable.grant_issuance(actor, grant_id, req, now).await
    }
    async fn read_granted(
        &self, actor: &Principal, thread_id: ThreadId, fence: &GrantFence, after_seq: u64, limit: usize,
    ) -> Result<(Thread, Grant, Vec<Message>)> {
        self.durable.read_granted(actor, thread_id, fence, after_seq, limit).await
    }
    async fn delivery_grant(
        &self, thread_id: ThreadId, recipient: &Principal, message_seq: u64, now: DateTime<Utc>,
    ) -> Result<DeliveryGrant> {
        self.durable.delivery_grant(thread_id, recipient, message_seq, now).await
    }

    async fn list_threads(
        &self,
        org_id: &str,
        scope: Option<&ScopeBinding>,
    ) -> Result<Vec<Thread>> {
        self.durable.list_threads(org_id, scope).await
    }

    async fn has_cap(&self, thread_id: ThreadId, principal: &Principal, cap: Cap) -> Result<bool> {
        self.durable.has_cap(thread_id, principal, cap).await
    }

    async fn add_participant(&self, thread_id: ThreadId, participant: Participant) -> Result<()> {
        self.durable.add_participant(thread_id, participant).await
    }

    async fn set_participant_role(
        &self,
        thread_id: ThreadId,
        principal: &Principal,
        role: Role,
    ) -> Result<()> {
        self.durable
            .set_participant_role(thread_id, principal, role)
            .await
    }

    async fn mutate_participant(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        target: Participant,
        create: bool,
    ) -> Result<()> {
        self.durable
            .mutate_participant(actor, thread_id, target, create)
            .await
    }

    async fn append_message(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
    ) -> Result<(Message, bool)> {
        if req.expected_grant_generation.is_some() || req.grant_fence.is_some() {
            // Buffered messages do not retain ingress authority. Commit scoped
            // writes directly so serialization cannot discard the generation.
            return self.durable.append_message(thread_id, sender, req).await;
        }
        let thread = self
            .durable
            .get_thread(thread_id)
            .await?
            .ok_or(Error::NotFound("thread"))?;

        if let Some(key) = req.idempotency_key.as_ref() {
            {
                let g = self.pending.lock().await;
                let map_key = (thread.org_id.clone(), key.clone());
                if let Some(existing) = g.idempotency.get(&map_key) {
                    if existing.thread_id != thread_id {
                        return Err(Error::Conflict("idempotency_key_reuse"));
                    }
                    return Ok((existing.clone(), false));
                }
            }
            let page = self.durable.read_messages(thread_id, 0, 10_000).await?;
            if let Some(existing) = page
                .iter()
                .find(|m| m.idempotency_key.as_ref() == Some(key))
            {
                return Ok((existing.clone(), false));
            }
        }

        let mut next_seq = {
            let g = self.pending.lock().await;
            g.next_seq.get(&thread_id).copied()
        };
        if next_seq.is_none() {
            let durable_msgs = self.durable.read_messages(thread_id, 0, 10_000).await?;
            let mut next = durable_msgs.last().map(|m| m.seq + 1).unwrap_or(1);
            let g = self.pending.lock().await;
            for item in g.queue.iter().filter(|i| i.message.thread_id == thread_id) {
                next = next.max(item.message.seq + 1);
            }
            next_seq = Some(next);
        }
        let seq = next_seq.unwrap();

        let message = Message {
            message_id: MessageId::new(),
            thread_id,
            seq,
            kind: req.kind,
            body: req.body,
            payload: if req.payload.is_null() {
                serde_json::json!({})
            } else {
                req.payload
            },
            sender: sender.clone(),
            idempotency_key: req.idempotency_key.clone(),
            correlation_id: req.correlation_id,
            parent_message_id: req.parent_message_id,
            causation_id: req.causation_id,
            created_at: Utc::now(),
        };

        let mut g = self.pending.lock().await;
        if let Some(key) = req.idempotency_key.as_ref() {
            let map_key = (thread.org_id.clone(), key.clone());
            if let Some(existing) = g.idempotency.get(&map_key) {
                return Ok((existing.clone(), false));
            }
        }
        // Re-read next_seq under lock in case of races.
        let seq = {
            let next = g.next_seq.get(&thread_id).copied().unwrap_or(seq);
            g.next_seq.insert(thread_id, next + 1);
            next
        };
        let mut message = message;
        message.seq = seq;

        if let Some(key) = message.idempotency_key.clone() {
            g.idempotency
                .insert((thread.org_id.clone(), key), message.clone());
        }

        let idx = g.queue.len();
        g.by_msg.insert(message.message_id, idx);
        g.queue.push_back(BufferedPublish {
            org_id: thread.org_id,
            message: message.clone(),
            recipients: Vec::new(),
        });
        // Mirror without recipients when Fabric has no fan-out; enqueue path mirrors later.
        Ok((message, true))
    }

    async fn append_with_delivery(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
        recipients: &[Principal],
    ) -> Result<(Message, bool)> {
        // Fabric acceptance always uses the durable authority. Legacy explicit batching
        // is retained for local experiments but cannot acknowledge product publishes.
        self.durable
            .append_with_delivery(thread_id, sender, req, recipients)
            .await
    }

    async fn read_messages(
        &self,
        thread_id: ThreadId,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut msgs = self
            .durable
            .read_messages(thread_id, after_seq, limit)
            .await?;
        let g = self.pending.lock().await;
        for item in g.queue.iter().filter(|i| i.message.thread_id == thread_id) {
            if item.message.seq > after_seq {
                msgs.push(item.message.clone());
            }
        }
        drop(g);
        msgs.sort_by_key(|m| m.seq);
        msgs.dedup_by_key(|m| m.message_id);
        msgs.truncate(limit);
        Ok(msgs)
    }

    async fn list_participants(&self, thread_id: ThreadId) -> Result<Vec<Participant>> {
        self.durable.list_participants(thread_id).await
    }

    async fn get_message(&self, message_id: MessageId) -> Result<Option<Message>> {
        self.durable.get_message(message_id).await
    }

    async fn enqueue_delivery_jobs(
        &self,
        message: &Message,
        recipients: &[Principal],
    ) -> Result<Vec<DeliveryJob>> {
        let pass_through = {
            let g = self.pending.lock().await;
            !g.by_msg.contains_key(&message.message_id)
        };
        if pass_through {
            return self
                .durable
                .enqueue_delivery_jobs(message, recipients)
                .await;
        }

        let mut g = self.pending.lock().await;
        let Some(&idx) = g.by_msg.get(&message.message_id) else {
            drop(g);
            return self
                .durable
                .enqueue_delivery_jobs(message, recipients)
                .await;
        };
        let Some(item) = g.queue.get_mut(idx) else {
            return Err(Error::NotFound("buffered_message"));
        };
        item.recipients = recipients.to_vec();
        let mirrored = item.clone();
        drop(g);
        self.mirror_item(&mirrored).await;
        Ok(recipients
            .iter()
            .map(|r| DeliveryJob {
                job_id: DeliveryJobId::new(),
                message_id: message.message_id,
                thread_id: message.thread_id,
                recipient: r.clone(),
                status: DeliveryStatus::Pending,
                attempts: 0,
                lease_until: None,
                next_attempt_at: None,
            })
            .collect())
    }

    async fn claim_delivery_jobs(&self, limit: usize) -> Result<Vec<DeliveryJob>> {
        self.durable.claim_delivery_jobs(limit).await
    }

    async fn settle_delivery_job(
        &self,
        job_id: DeliveryJobId,
        expected_attempt: u32,
        status: DeliveryStatus,
    ) -> Result<()> {
        self.durable
            .settle_delivery_job(job_id, expected_attempt, status)
            .await
    }

    async fn flush_write_batch(&self, batch: &[BufferedPublish]) -> Result<()> {
        self.durable.flush_write_batch(batch).await
    }
}
