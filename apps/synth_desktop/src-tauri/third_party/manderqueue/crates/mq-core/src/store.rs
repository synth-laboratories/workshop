use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::batch::BufferedPublish;
use crate::error::Result;
use crate::grants::*;
use crate::types::*;

#[async_trait]
pub trait Store: Send + Sync {
    async fn insert_thread(&self, creator: &Principal, req: CreateThread) -> Result<Thread>;
    /// Lookup by org-scoped thread idempotency key (ensure / retry).
    async fn find_thread_by_idempotency(
        &self,
        org_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<Thread>>;
    async fn get_thread(&self, thread_id: ThreadId) -> Result<Option<Thread>>;
    /// Read a bounded snapshot and validate scoped membership under the same lock.
    /// Implementations must not emulate this with separate authorization/read calls.
    async fn read_scoped(
        &self, actor: &Principal, thread_id: ThreadId, generation: u64,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Vec<Message>)> {
        let _ = (actor, thread_id, generation, after_seq, limit);
        Err(crate::Error::Invalid("atomic_scoped_read_unsupported"))
    }
    async fn list_threads(&self, org_id: &str, scope: Option<&ScopeBinding>)
        -> Result<Vec<Thread>>;

    // ---- Enrollment and grants (docs/WORKSHOP_GRANT_CONTRACT.md) ----
    // Unsupported adapters refuse; none may emulate atomic checks with
    // separate authorization and data calls.

    /// Create or re-enroll (advancing the incarnation) under one statement.
    async fn enroll(&self, owner: &Principal, req: EnrollDevice, now: DateTime<Utc>) -> Result<Enrollment> {
        let _ = (owner, req, now);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    async fn get_enrollment(&self, enrollment_id: Uuid) -> Result<Option<Enrollment>> {
        let _ = enrollment_id;
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    async fn list_enrollments(&self, owner: &Principal) -> Result<Vec<Enrollment>> {
        let _ = owner;
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    /// Device sign-out (owner only): mark the enrollment revoked, revoke all
    /// of its active grants (generation +1) and dead-letter every pending job
    /// for its principal, in one transaction. Idempotent.
    async fn revoke_enrollment(&self, owner: &Principal, enrollment_id: Uuid, now: DateTime<Utc>) -> Result<Enrollment> {
        let _ = (owner, enrollment_id, now);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    /// Authorize (invite + enrollment owner), add grantee membership if absent
    /// and insert the grant under the thread lock.
    async fn create_grant(&self, actor: &Principal, req: CreateGrant, now: DateTime<Utc>) -> Result<Grant> {
        let _ = (actor, req, now);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    /// Raw grant (with live incarnation) plus its enrollment; callers check visibility.
    async fn get_grant(&self, grant_id: Uuid, now: DateTime<Utc>) -> Result<Option<(Grant, Enrollment)>> {
        let _ = (grant_id, now);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    async fn list_grants(&self, org_id: &str, filter: &GrantFilter, now: DateTime<Utc>) -> Result<Vec<(Grant, Enrollment)>> {
        let _ = (org_id, filter, now);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    /// Authorize and apply revoke/restore/renew atomically. Revoke also
    /// dead-letters the grantee's pending jobs on the thread.
    async fn mutate_grant(&self, actor: &Principal, grant_id: Uuid, mutation: GrantMutation, now: DateTime<Utc>) -> Result<Grant> {
        let _ = (actor, grant_id, mutation, now);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    /// Live authority snapshot for credential issuance.
    async fn grant_issuance(&self, actor: &Principal, grant_id: Uuid, req: GrantIssuanceRequest, now: DateTime<Utc>) -> Result<Grant> {
        let _ = (actor, grant_id, req, now);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    /// Check a grant credential and read at most `limit` messages with
    /// `seq > max(after_seq, floor)` in one locked snapshot.
    async fn read_granted(
        &self, actor: &Principal, thread_id: ThreadId, fence: &GrantFence,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Grant, Vec<Message>)> {
        let _ = (actor, thread_id, fence, after_seq, limit);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    /// Pre-dispatch recheck for one queued job to an enrollment principal.
    async fn delivery_grant(
        &self, thread_id: ThreadId, recipient: &Principal, message_seq: u64, now: DateTime<Utc>,
    ) -> Result<DeliveryGrant> {
        let _ = (thread_id, recipient, message_seq, now);
        Err(crate::Error::Invalid("grants_unsupported"))
    }
    async fn has_cap(&self, thread_id: ThreadId, principal: &Principal, cap: Cap) -> Result<bool>;
    async fn add_participant(&self, thread_id: ThreadId, participant: Participant) -> Result<()>;
    async fn set_participant_role(
        &self,
        thread_id: ThreadId,
        principal: &Principal,
        role: Role,
    ) -> Result<()>;
    /// Authorize and mutate under one storage transaction. Same-role invite replay is a no-op.
    async fn mutate_participant(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        target: Participant,
        create: bool,
    ) -> Result<()> {
        let _ = (actor, thread_id, target, create);
        Err(crate::Error::Invalid("atomic_membership_unsupported"))
    }
    async fn append_message(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
    ) -> Result<(Message, bool)>;
    /// Commit message and frozen recipient intent atomically; replay repairs missing jobs only.
    /// Unsupported adapters refuse rather than simulate atomicity with separate commits.
    async fn append_with_delivery(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
        recipients: &[Principal],
    ) -> Result<(Message, bool)> {
        let _ = (thread_id, sender, req, recipients);
        Err(crate::Error::Invalid("atomic_publish_unsupported"))
    }
    async fn read_messages(
        &self,
        thread_id: ThreadId,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<Message>>;
    async fn list_participants(&self, thread_id: ThreadId) -> Result<Vec<Participant>>;
    async fn get_message(&self, message_id: MessageId) -> Result<Option<Message>>;
    async fn enqueue_delivery_jobs(
        &self,
        message: &Message,
        recipients: &[Principal],
    ) -> Result<Vec<DeliveryJob>>;
    async fn claim_delivery_jobs(&self, limit: usize) -> Result<Vec<DeliveryJob>>;
    async fn settle_delivery_job(
        &self,
        job_id: DeliveryJobId,
        expected_attempt: u32,
        status: DeliveryStatus,
    ) -> Result<()>;

    /// Persist already-formed publishes (message + jobs) in as few durable
    /// transactions as possible. Used by [`crate::BatchingStore`] flush.
    ///
    /// Default: one `append`-equivalent insert + enqueue per item (higher PG load).
    async fn flush_write_batch(&self, batch: &[BufferedPublish]) -> Result<()> {
        for item in batch {
            let req = PublishMessage {
                expected_grant_generation: None,
                grant_fence: None,
                kind: item.message.kind,
                body: item.message.body.clone(),
                payload: item.message.payload.clone(),
                idempotency_key: item.message.idempotency_key.clone(),
                correlation_id: item.message.correlation_id.clone(),
                parent_message_id: item.message.parent_message_id,
                causation_id: item.message.causation_id.clone(),
                recipients: Vec::new(),
            };
            let (msg, created) = self
                .append_message(item.message.thread_id, &item.message.sender, req)
                .await?;
            if created && !item.recipients.is_empty() {
                let _ = self.enqueue_delivery_jobs(&msg, &item.recipients).await?;
            }
        }
        Ok(())
    }
}
