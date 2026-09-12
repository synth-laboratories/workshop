use async_trait::async_trait;

use crate::batch::BufferedPublish;
use crate::error::Result;
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
    async fn list_threads(&self, org_id: &str, scope: Option<&ScopeBinding>)
        -> Result<Vec<Thread>>;
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
