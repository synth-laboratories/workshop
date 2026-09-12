use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::batch::BufferedPublish;
use crate::error::{Error, Result};
use crate::store::Store;
use crate::types::*;

#[derive(Clone, Default)]
pub struct MemoryStore {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    threads: HashMap<ThreadId, Thread>,
    /// (org_id, idempotency_key) → thread_id
    thread_idempotency: HashMap<(String, String), ThreadId>,
    participants: HashMap<ThreadId, Vec<Participant>>,
    messages: HashMap<ThreadId, Vec<Message>>,
    idempotency: HashMap<(String, String), Message>,
    jobs: HashMap<DeliveryJobId, DeliveryJob>,
    acceptances: HashMap<MessageId, PublishAcceptance>,
}

/// Whole-instance transport state for isolated, embedded containers only.
/// Restore into a new store, never overwrite a running fabric.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCheckpoint {
    pub version: u32,
    pub threads: Vec<(Thread, Vec<Participant>, Vec<Message>)>,
    pub jobs: Vec<DeliveryJob>,
    #[serde(default)]
    pub acceptances: Vec<PublishAcceptance>,
}

impl MemoryStore {
    pub fn checkpoint(&self) -> MemoryCheckpoint {
        let g = self.inner.lock().expect("lock");
        let mut threads: Vec<_> = g
            .threads
            .values()
            .map(|t| {
                (
                    t.clone(),
                    g.participants[&t.thread_id].clone(),
                    g.messages[&t.thread_id].clone(),
                )
            })
            .collect();
        threads.sort_by_key(|t| t.0.thread_id.0);
        let mut jobs: Vec<_> = g.jobs.values().cloned().collect();
        jobs.sort_by_key(|j| j.job_id.0);
        let mut acceptances: Vec<_> = g.acceptances.values().cloned().collect();
        acceptances.sort_by_key(|a| a.message_id.0);
        MemoryCheckpoint {
            version: 2,
            threads,
            jobs,
            acceptances,
        }
    }

    pub fn from_checkpoint(snapshot: MemoryCheckpoint) -> Result<Self> {
        if !matches!(snapshot.version, 1 | 2) {
            return Err(Error::Invalid("checkpoint_version"));
        }
        let mut g = Inner::default();
        let mut message_ids = std::collections::HashSet::new();
        for (thread, participants, messages) in snapshot.threads {
            let tid = thread.thread_id;
            if g.threads.contains_key(&tid)
                || participants
                    .iter()
                    .filter(|p| p.role == Role::Owner)
                    .count()
                    != 1
            {
                return Err(Error::Invalid("checkpoint_thread"));
            }
            let mut principals = std::collections::HashSet::new();
            if participants.iter().any(|p| {
                p.principal.org_id != thread.org_id
                    || p.caps != p.role.caps()
                    || !principals.insert(p.principal.clone())
            }) {
                return Err(Error::Invalid("checkpoint_participants"));
            }
            if let Some(key) = &thread.idempotency_key {
                if g.thread_idempotency
                    .insert((thread.org_id.clone(), key.clone()), tid)
                    .is_some()
                {
                    return Err(Error::Invalid("checkpoint_thread_idempotency"));
                }
            }
            for (i, m) in messages.iter().enumerate() {
                if m.thread_id != tid
                    || m.seq != i as u64 + 1
                    || m.sender.org_id != thread.org_id
                    || !message_ids.insert(m.message_id)
                {
                    return Err(Error::Invalid("checkpoint_message"));
                }
                if let Some(key) = &m.idempotency_key {
                    if g.idempotency
                        .insert(publish_key(tid, &m.sender, key), m.clone())
                        .is_some()
                    {
                        return Err(Error::Invalid("checkpoint_message_idempotency"));
                    }
                }
            }
            g.participants.insert(tid, participants);
            g.messages.insert(tid, messages);
            g.threads.insert(tid, thread);
        }
        for job in snapshot.jobs {
            let messages = g
                .messages
                .get(&job.thread_id)
                .ok_or(Error::Invalid("checkpoint_job_thread"))?;
            if !messages.iter().any(|m| m.message_id == job.message_id)
                || !g.participants[&job.thread_id]
                    .iter()
                    .any(|p| p.principal == job.recipient)
                || g.jobs.insert(job.job_id, job).is_some()
            {
                return Err(Error::Invalid("checkpoint_job"));
            }
        }
        for acceptance in snapshot.acceptances {
            if !message_ids.contains(&acceptance.message_id)
                || g.acceptances
                    .insert(acceptance.message_id, acceptance)
                    .is_some()
            {
                return Err(Error::Invalid("checkpoint_acceptance"));
            }
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(g)),
        })
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn insert_thread(&self, _creator: &Principal, req: CreateThread) -> Result<Thread> {
        let mut g = self.inner.lock().expect("lock");
        if let Some(key) = req.idempotency_key.as_ref() {
            let map_key = (req.org_id.clone(), key.clone());
            if let Some(tid) = g.thread_idempotency.get(&map_key) {
                return Ok(g.threads.get(tid).expect("thread").clone());
            }
        }
        let thread = Thread {
            thread_id: ThreadId::new(),
            org_id: req.org_id.clone(),
            scope: req.scope,
            title: req.title,
            idempotency_key: req.idempotency_key.clone(),
            created_at: Utc::now(),
        };
        if let Some(key) = req.idempotency_key {
            g.thread_idempotency
                .insert((req.org_id, key), thread.thread_id);
        }
        g.participants.insert(thread.thread_id, req.participants);
        g.messages.insert(thread.thread_id, Vec::new());
        g.threads.insert(thread.thread_id, thread.clone());
        Ok(thread)
    }

    async fn find_thread_by_idempotency(
        &self,
        org_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<Thread>> {
        let g = self.inner.lock().expect("lock");
        Ok(g.thread_idempotency
            .get(&(org_id.to_string(), idempotency_key.to_string()))
            .and_then(|tid| g.threads.get(tid).cloned()))
    }

    async fn get_thread(&self, thread_id: ThreadId) -> Result<Option<Thread>> {
        Ok(self
            .inner
            .lock()
            .expect("lock")
            .threads
            .get(&thread_id)
            .cloned())
    }

    async fn list_threads(
        &self,
        org_id: &str,
        scope: Option<&ScopeBinding>,
    ) -> Result<Vec<Thread>> {
        let g = self.inner.lock().expect("lock");
        Ok(g.threads
            .values()
            .filter(|t| t.org_id == org_id)
            .filter(|t| scope.is_none_or(|s| &t.scope == s))
            .cloned()
            .collect())
    }

    async fn has_cap(&self, thread_id: ThreadId, principal: &Principal, cap: Cap) -> Result<bool> {
        let g = self.inner.lock().expect("lock");
        Ok(g.participants
            .get(&thread_id)
            .and_then(|ps| {
                ps.iter().find(|p| {
                    p.principal.kind == principal.kind
                        && p.principal.id == principal.id
                        && p.principal.org_id == principal.org_id
                })
            })
            .is_some_and(|p| p.caps.contains(&cap)))
    }

    async fn add_participant(&self, thread_id: ThreadId, participant: Participant) -> Result<()> {
        let mut g = self.inner.lock().expect("lock");
        let Some(ps) = g.participants.get_mut(&thread_id) else {
            return Err(Error::NotFound("thread"));
        };
        if ps.iter().any(|p| p.principal == participant.principal) {
            return Err(Error::Conflict("participant_exists"));
        }
        ps.push(participant.normalize());
        Ok(())
    }

    async fn set_participant_role(
        &self,
        thread_id: ThreadId,
        principal: &Principal,
        role: Role,
    ) -> Result<()> {
        let mut g = self.inner.lock().expect("lock");
        let Some(ps) = g.participants.get_mut(&thread_id) else {
            return Err(Error::NotFound("thread"));
        };
        let Some(p) = ps.iter_mut().find(|p| p.principal == *principal) else {
            return Err(Error::NotFound("participant"));
        };
        p.role = role;
        p.caps = role.caps();
        Ok(())
    }

    async fn mutate_participant(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        target: Participant,
        create: bool,
    ) -> Result<()> {
        let mut g = self.inner.lock().expect("lock");
        let org = g
            .threads
            .get(&thread_id)
            .ok_or(Error::NotFound("thread"))?
            .org_id
            .clone();
        let members = g
            .participants
            .get_mut(&thread_id)
            .ok_or(Error::NotFound("thread"))?;
        let target = target.normalize();
        if !validate_participant_change(&org, actor, members, &target, create)? {
            return Ok(());
        }
        let revoked = (target.role == Role::Revoked).then(|| target.principal.clone());
        if let Some(existing) = members.iter_mut().find(|p| p.principal == target.principal) {
            *existing = target;
        } else {
            members.push(target);
        }
        if let Some(principal) = revoked {
            for job in g.jobs.values_mut() {
                if job.thread_id == thread_id && job.recipient == principal
                    && job.status == DeliveryStatus::Pending
                {
                    job.status = DeliveryStatus::DeadLetter;
                    job.lease_until = None;
                    job.next_attempt_at = None;
                }
            }
        }
        Ok(())
    }

    async fn append_message(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
    ) -> Result<(Message, bool)> {
        self.append_with_delivery(thread_id, sender, req, &[]).await
    }

    async fn append_with_delivery(
        &self,
        thread_id: ThreadId,
        sender: &Principal,
        req: PublishMessage,
        recipients: &[Principal],
    ) -> Result<(Message, bool)> {
        let mut g = self.inner.lock().expect("lock");
        let thread = g
            .threads
            .get(&thread_id)
            .ok_or(Error::NotFound("thread"))?
            .clone();

        if thread.org_id != sender.org_id {
            return Err(Error::Forbidden("org_workspace_mismatch"));
        }
        if recipients.iter().any(|r| {
            r.org_id != thread.org_id
                || !g.participants[&thread_id]
                    .iter()
                    .any(|p| p.principal == *r && p.caps.contains(&Cap::Read))
        }) {
            return Err(Error::Invalid("recipient_not_a_member"));
        }
        if !g.participants[&thread_id]
            .iter()
            .any(|p| p.principal == *sender && p.caps.contains(&Cap::Publish))
        {
            return Err(Error::Forbidden("publish_membership_required"));
        }
        let fingerprint = publish_fingerprint(&req);
        if let Some(key) = req.idempotency_key.as_ref() {
            let map_key = publish_key(thread_id, sender, key);
            if let Some(existing) = g.idempotency.get(&map_key) {
                if existing.thread_id != thread_id {
                    return Err(Error::Conflict("idempotency_key_reuse"));
                }
                let existing = existing.clone();
                let acceptance = g
                    .acceptances
                    .get(&existing.message_id)
                    .ok_or(Error::Conflict("legacy_publish_unverifiable"))?
                    .clone();
                if acceptance.fingerprint != fingerprint {
                    return Err(Error::Conflict("idempotency_payload_mismatch"));
                }
                insert_missing_jobs(&mut g, &existing, &acceptance.recipients);
                return Ok((existing, false));
            }
        }

        let seq = g
            .messages
            .get(&thread_id)
            .ok_or(Error::NotFound("thread"))?
            .last()
            .map(|m| m.seq + 1)
            .unwrap_or(1);
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
        if let Some(key) = message.idempotency_key.clone() {
            g.idempotency
                .insert(publish_key(thread_id, sender, &key), message.clone());
        }
        g.messages
            .get_mut(&thread_id)
            .ok_or(Error::NotFound("thread"))?
            .push(message.clone());
        g.acceptances.insert(
            message.message_id,
            PublishAcceptance {
                message_id: message.message_id,
                fingerprint,
                recipients: recipients.to_vec(),
            },
        );
        insert_missing_jobs(&mut g, &message, recipients);
        Ok((message, true))
    }

    async fn read_messages(
        &self,
        thread_id: ThreadId,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let g = self.inner.lock().expect("lock");
        Ok(g.messages
            .get(&thread_id)
            .into_iter()
            .flatten()
            .filter(|m| m.seq > after_seq)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn list_participants(&self, thread_id: ThreadId) -> Result<Vec<Participant>> {
        let g = self.inner.lock().expect("lock");
        Ok(g.participants.get(&thread_id).cloned().unwrap_or_default())
    }

    async fn get_message(&self, message_id: MessageId) -> Result<Option<Message>> {
        let g = self.inner.lock().expect("lock");
        for msgs in g.messages.values() {
            if let Some(m) = msgs.iter().find(|m| m.message_id == message_id) {
                return Ok(Some(m.clone()));
            }
        }
        Ok(None)
    }

    async fn enqueue_delivery_jobs(
        &self,
        message: &Message,
        recipients: &[Principal],
    ) -> Result<Vec<DeliveryJob>> {
        let mut g = self.inner.lock().expect("lock");
        let mut out = Vec::new();
        for recipient in recipients {
            let job = DeliveryJob {
                job_id: DeliveryJobId::new(),
                message_id: message.message_id,
                thread_id: message.thread_id,
                recipient: recipient.clone(),
                status: DeliveryStatus::Pending,
                attempts: 0,
                lease_until: None,
                next_attempt_at: None,
            };
            g.jobs.insert(job.job_id, job.clone());
            out.push(job);
        }
        Ok(out)
    }

    async fn claim_delivery_jobs(&self, limit: usize) -> Result<Vec<DeliveryJob>> {
        let mut g = self.inner.lock().expect("lock");
        let mut claimed = Vec::new();
        for job in g.jobs.values_mut() {
            if claimed.len() >= limit {
                break;
            }
            let now = Utc::now();
            if job.status == DeliveryStatus::Pending
                && job.lease_until.is_none_or(|until| until <= now)
                && job.next_attempt_at.is_none_or(|at| at <= now)
            {
                job.attempts += 1;
                job.lease_until = Some(now + chrono::Duration::seconds(DELIVERY_LEASE_SECONDS));
                claimed.push(job.clone());
            }
        }
        Ok(claimed)
    }

    async fn settle_delivery_job(
        &self,
        job_id: DeliveryJobId,
        expected_attempt: u32,
        status: DeliveryStatus,
    ) -> Result<()> {
        let mut g = self.inner.lock().expect("lock");
        let job = g.jobs.get_mut(&job_id).ok_or(Error::NotFound("job"))?;
        let now = Utc::now();
        if job.status != DeliveryStatus::Pending
            || job.attempts != expected_attempt
            || job.lease_until.is_none_or(|until| until <= now)
        {
            return Err(Error::Conflict("stale_delivery_claim"));
        }
        job.status = status;
        job.lease_until = None;
        job.next_attempt_at = if status == DeliveryStatus::Pending {
            Some(now + chrono::Duration::seconds(delivery_backoff_seconds(expected_attempt)))
        } else {
            None
        };
        Ok(())
    }

    async fn flush_write_batch(&self, batch: &[BufferedPublish]) -> Result<()> {
        let mut g = self.inner.lock().expect("lock");
        for item in batch {
            if !g.messages.contains_key(&item.message.thread_id) {
                return Err(Error::NotFound("thread"));
            }
            if let Some(key) = item.message.idempotency_key.as_ref() {
                let map_key = (item.org_id.clone(), key.clone());
                if let Some(existing) = g.idempotency.get(&map_key) {
                    if existing.message_id == item.message.message_id {
                        // already applied
                    } else {
                        return Err(Error::Conflict("idempotency_key_reuse"));
                    }
                } else {
                    g.idempotency.insert(map_key, item.message.clone());
                }
            }
            let msgs = g.messages.get_mut(&item.message.thread_id).unwrap();
            if !msgs.iter().any(|m| m.message_id == item.message.message_id) {
                msgs.push(item.message.clone());
            }
            for recipient in &item.recipients {
                let job = DeliveryJob {
                    job_id: DeliveryJobId::new(),
                    message_id: item.message.message_id,
                    thread_id: item.message.thread_id,
                    recipient: recipient.clone(),
                    status: DeliveryStatus::Pending,
                    attempts: 0,
                    lease_until: None,
                    next_attempt_at: None,
                };
                g.jobs.insert(job.job_id, job);
            }
        }
        Ok(())
    }
}

fn insert_missing_jobs(g: &mut Inner, message: &Message, recipients: &[Principal]) {
    for recipient in recipients {
        if g.jobs
            .values()
            .any(|j| j.message_id == message.message_id && j.recipient == *recipient)
        {
            continue;
        }
        let job = DeliveryJob {
            job_id: DeliveryJobId::new(),
            message_id: message.message_id,
            thread_id: message.thread_id,
            recipient: recipient.clone(),
            status: DeliveryStatus::Pending,
            attempts: 0,
            lease_until: None,
            next_attempt_at: None,
        };
        g.jobs.insert(job.job_id, job);
    }
}
