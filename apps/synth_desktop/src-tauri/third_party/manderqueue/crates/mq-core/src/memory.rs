use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use chrono::DateTime;
use uuid::Uuid;

use crate::batch::BufferedPublish;
use crate::error::{Error, Result};
use crate::grants::*;
use crate::store::Store;
use crate::types::*;

type EnrollmentKey = (String, String, String, String);

fn enrollment_key(owner: &Principal, device_id: &str, session_id: &str) -> EnrollmentKey {
    (
        owner.org_id.clone(),
        serde_json::to_string(owner).expect("principal serialization"),
        device_id.to_string(),
        session_id.to_string(),
    )
}

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
    enrollments: HashMap<Uuid, Enrollment>,
    enrollment_keys: HashMap<EnrollmentKey, Uuid>,
    grants: HashMap<Uuid, Grant>,
}

impl Inner {
    fn grant_parts(&self, grant_id: Uuid) -> Option<(&Grant, &Enrollment, &[Participant])> {
        let grant = self.grants.get(&grant_id)?;
        let enrollment = self.enrollments.get(&grant.enrollment_id)?;
        let members = self.participants.get(&grant.thread_id).map(Vec::as_slice).unwrap_or(&[]);
        Some((grant, enrollment, members))
    }
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
    /// Version 3+. Absent in older checkpoints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enrollments: Vec<Enrollment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<Grant>,
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
        let mut enrollments: Vec<_> = g.enrollments.values().cloned().collect();
        enrollments.sort_by_key(|e| e.enrollment_id);
        let mut grants: Vec<_> = g.grants.values().cloned().collect();
        grants.sort_by_key(|x| x.grant_id);
        MemoryCheckpoint {
            // Older readers stay compatible with checkpoints that carry no grants.
            version: if enrollments.is_empty() && grants.is_empty() { 2 } else { 3 },
            threads,
            jobs,
            acceptances,
            enrollments,
            grants,
        }
    }

    pub fn from_checkpoint(snapshot: MemoryCheckpoint) -> Result<Self> {
        if !matches!(snapshot.version, 1..=3)
            || (snapshot.version < 3 && (!snapshot.enrollments.is_empty() || !snapshot.grants.is_empty()))
        {
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
        for enrollment in snapshot.enrollments {
            let key = enrollment_key(&enrollment.owner, &enrollment.device_id, &enrollment.session_id);
            if enrollment.principal != enrollment_principal(&enrollment.org_id, enrollment.enrollment_id)
                || enrollment.owner.org_id != enrollment.org_id
                || enrollment.incarnation == 0
                || g.enrollment_keys.insert(key, enrollment.enrollment_id).is_some()
                || g.enrollments.insert(enrollment.enrollment_id, enrollment).is_some()
            {
                return Err(Error::Invalid("checkpoint_enrollment"));
            }
        }
        let mut pairs = std::collections::HashSet::new();
        for grant in snapshot.grants {
            let valid = g.threads.get(&grant.thread_id).is_some_and(|t| t.org_id == grant.org_id)
                && g.enrollments.get(&grant.enrollment_id).is_some_and(|e| {
                    e.org_id == grant.org_id && e.principal == grant.principal
                })
                && g.participants[&grant.thread_id].iter().any(|p| p.principal == grant.principal)
                && validate_operations(&grant.operations).is_ok()
                && pairs.insert((grant.thread_id, grant.enrollment_id));
            if !valid || g.grants.insert(grant.grant_id, grant).is_some() {
                return Err(Error::Invalid("checkpoint_grant"));
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

    async fn read_scoped(
        &self, actor: &Principal, thread_id: ThreadId, generation: u64,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Vec<Message>)> {
        let g = self.inner.lock().expect("lock");
        let thread = g.threads.get(&thread_id).ok_or(Error::NotFound("thread"))?;
        if thread.org_id != actor.org_id {
            return Err(Error::NotFound("thread"));
        }
        let authorized = g.participants.get(&thread_id).into_iter().flatten().any(|p| {
            p.principal == *actor && p.grant_generation == generation
                && p.role != Role::Revoked && p.caps.contains(&Cap::Read)
        });
        if !authorized {
            return Err(Error::Forbidden("stale_grant_generation"));
        }
        let messages = g.messages.get(&thread_id).into_iter().flatten()
            .filter(|m| m.seq > after_seq).take(limit.min(HISTORY_MAX_LIMIT + 1)).cloned().collect();
        Ok((thread.clone(), messages))
    }

    async fn enroll(&self, owner: &Principal, req: EnrollDevice, now: DateTime<Utc>) -> Result<Enrollment> {
        let mut g = self.inner.lock().expect("lock");
        let key = enrollment_key(owner, &req.device_id, &req.session_id);
        if let Some(id) = g.enrollment_keys.get(&key).copied() {
            let enrollment = g.enrollments.get_mut(&id).expect("enrollment");
            if enrollment.is_revoked() {
                return Err(Error::Forbidden("enrollment_revoked"));
            }
            enrollment.incarnation = enrollment
                .incarnation
                .checked_add(1)
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or(Error::Invalid("incarnation_exhausted"))?;
            if req.label.is_some() {
                enrollment.label = req.label;
            }
            enrollment.updated_at = now;
            return Ok(enrollment.clone());
        }
        let enrollment_id = Uuid::new_v4();
        let enrollment = Enrollment {
            enrollment_id,
            org_id: owner.org_id.clone(),
            owner: owner.clone(),
            device_id: req.device_id,
            session_id: req.session_id,
            label: req.label,
            principal: enrollment_principal(&owner.org_id, enrollment_id),
            incarnation: 1,
            revoked_at: None,
            created_at: now,
            updated_at: now,
        };
        g.enrollment_keys.insert(key, enrollment_id);
        g.enrollments.insert(enrollment_id, enrollment.clone());
        Ok(enrollment)
    }

    async fn get_enrollment(&self, enrollment_id: Uuid) -> Result<Option<Enrollment>> {
        Ok(self.inner.lock().expect("lock").enrollments.get(&enrollment_id).cloned())
    }

    async fn list_enrollments(&self, owner: &Principal) -> Result<Vec<Enrollment>> {
        let g = self.inner.lock().expect("lock");
        let mut out: Vec<_> = g.enrollments.values().filter(|e| e.owner == *owner).cloned().collect();
        out.sort_by_key(|e| (e.created_at, e.enrollment_id));
        Ok(out)
    }

    async fn revoke_enrollment(&self, owner: &Principal, enrollment_id: Uuid, now: DateTime<Utc>) -> Result<Enrollment> {
        let mut g = self.inner.lock().expect("lock");
        let enrollment = g.enrollments.get(&enrollment_id).cloned().ok_or(Error::NotFound("enrollment"))?;
        if enrollment.owner != *owner {
            return Err(Error::NotFound("enrollment"));
        }
        if enrollment.is_revoked() {
            return Ok(enrollment);
        }
        // One critical section: grants, queued jobs and the enrollment itself.
        revoke_enrollment_grants(g.grants.values_mut().filter(|x| x.enrollment_id == enrollment_id), now)?;
        for job in g.jobs.values_mut() {
            if job.recipient == enrollment.principal && job.status == DeliveryStatus::Pending {
                job.status = DeliveryStatus::DeadLetter;
                job.lease_until = None;
                job.next_attempt_at = None;
            }
        }
        let stored = g.enrollments.get_mut(&enrollment_id).expect("enrollment");
        stored.revoked_at = Some(now);
        stored.updated_at = now;
        Ok(stored.clone())
    }

    async fn create_grant(&self, actor: &Principal, req: CreateGrant, now: DateTime<Utc>) -> Result<Grant> {
        let mut g = self.inner.lock().expect("lock");
        let thread = g.threads.get(&req.thread_id).ok_or(Error::NotFound("thread"))?.clone();
        if thread.org_id != actor.org_id {
            return Err(Error::NotFound("thread"));
        }
        let enrollment = g.enrollments.get(&req.enrollment_id).cloned().ok_or(Error::NotFound("enrollment"))?;
        let members = g.participants.get(&req.thread_id).cloned().unwrap_or_default();
        let head = g.messages.get(&req.thread_id).and_then(|m| m.last()).map(|m| m.seq).unwrap_or(0);
        let (floor, add) = check_grant_create(actor, &thread, &members, &enrollment, &req, head)?;
        if g.grants.values().any(|x| x.thread_id == req.thread_id && x.enrollment_id == req.enrollment_id) {
            return Err(Error::Conflict("grant_exists"));
        }
        if let Some(participant) = add {
            g.participants.get_mut(&req.thread_id).ok_or(Error::NotFound("thread"))?.push(participant);
        }
        let grant = Grant {
            grant_id: Uuid::new_v4(),
            org_id: thread.org_id.clone(),
            thread_id: req.thread_id,
            enrollment_id: enrollment.enrollment_id,
            principal: enrollment.principal.clone(),
            operations: req.operations,
            history_after_seq: floor,
            expires_at: now + chrono::Duration::seconds(req.ttl_seconds),
            incarnation: enrollment.incarnation,
            generation: 0,
            status: GrantStatus::Active,
            state: GrantState::Active,
            granted_by: actor.clone(),
            created_at: now,
            updated_at: now,
        };
        g.grants.insert(grant.grant_id, grant.clone());
        Ok(grant.view(&enrollment, now))
    }

    async fn get_grant(&self, grant_id: Uuid, now: DateTime<Utc>) -> Result<Option<(Grant, Enrollment)>> {
        let g = self.inner.lock().expect("lock");
        Ok(g.grant_parts(grant_id).map(|(grant, enrollment, _)| {
            (grant.clone().view(enrollment, now), enrollment.clone())
        }))
    }

    async fn list_grants(&self, org_id: &str, filter: &GrantFilter, now: DateTime<Utc>) -> Result<Vec<(Grant, Enrollment)>> {
        let g = self.inner.lock().expect("lock");
        let mut out: Vec<_> = g
            .grants
            .values()
            .filter(|x| x.org_id == org_id)
            .filter(|x| filter.enrollment_id.is_none_or(|e| e == x.enrollment_id))
            .filter(|x| filter.thread_id.is_none_or(|t| t == x.thread_id))
            .filter_map(|x| {
                let enrollment = g.enrollments.get(&x.enrollment_id)?;
                Some((x.clone().view(enrollment, now), enrollment.clone()))
            })
            .collect();
        out.sort_by_key(|(x, _)| (x.created_at, x.grant_id));
        Ok(out)
    }

    async fn mutate_grant(&self, actor: &Principal, grant_id: Uuid, mutation: GrantMutation, now: DateTime<Utc>) -> Result<Grant> {
        let mut g = self.inner.lock().expect("lock");
        let (grant, enrollment, members) = g.grant_parts(grant_id).ok_or(Error::NotFound("grant"))?;
        if grant.org_id != actor.org_id {
            return Err(Error::NotFound("grant"));
        }
        let (current, enrollment) = (grant.clone().view(enrollment, now), enrollment.clone());
        let Some(next) = check_grant_mutation(actor, &current, &enrollment, members, mutation, now)? else {
            return Ok(current);
        };
        if matches!(mutation, GrantMutation::Revoke) {
            for job in g.jobs.values_mut() {
                if job.thread_id == current.thread_id && job.recipient == current.principal
                    && job.status == DeliveryStatus::Pending
                {
                    job.status = DeliveryStatus::DeadLetter;
                    job.lease_until = None;
                    job.next_attempt_at = None;
                }
            }
        }
        g.grants.insert(grant_id, next.clone());
        Ok(next.view(&enrollment, now))
    }

    async fn grant_issuance(&self, actor: &Principal, grant_id: Uuid, req: GrantIssuanceRequest, now: DateTime<Utc>) -> Result<Grant> {
        let g = self.inner.lock().expect("lock");
        let (grant, enrollment, members) = g.grant_parts(grant_id).ok_or(Error::NotFound("grant"))?;
        if grant.org_id != actor.org_id {
            return Err(Error::NotFound("grant"));
        }
        let current = grant.clone().view(enrollment, now);
        check_grant_issuance(actor, &current, enrollment, members, &req, now)?;
        Ok(current)
    }

    async fn read_granted(
        &self, actor: &Principal, thread_id: ThreadId, fence: &GrantFence,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Grant, Vec<Message>)> {
        let g = self.inner.lock().expect("lock");
        let thread = g.threads.get(&thread_id).ok_or(Error::NotFound("thread"))?;
        if thread.org_id != actor.org_id {
            return Err(Error::NotFound("thread"));
        }
        let (grant, enrollment, members) =
            g.grant_parts(fence.grant_id).ok_or(Error::Forbidden("grant_operation_denied"))?;
        check_grant_access(actor, thread_id, grant, enrollment, members, fence, GrantOperation::Read)?;
        let effective = after_seq.max(grant.history_after_seq);
        let messages = g.messages.get(&thread_id).into_iter().flatten()
            .filter(|m| m.seq > effective)
            .take(limit.min(HISTORY_MAX_LIMIT + 1))
            .cloned()
            .collect();
        Ok((thread.clone(), grant.clone().view(enrollment, fence.at), messages))
    }

    async fn delivery_grant(
        &self, thread_id: ThreadId, recipient: &Principal, message_seq: u64, now: DateTime<Utc>,
    ) -> Result<DeliveryGrant> {
        if !is_enrollment_principal(recipient) {
            return Ok(DeliveryGrant::NotGoverned);
        }
        let g = self.inner.lock().expect("lock");
        let grant = g.grants.values().find(|x| x.thread_id == thread_id && x.principal == *recipient);
        let enrollment = grant.and_then(|x| g.enrollments.get(&x.enrollment_id));
        let members = g.participants.get(&thread_id).map(Vec::as_slice).unwrap_or(&[]);
        Ok(match (grant, enrollment) {
            (Some(grant), Some(enrollment)) if delivery_allowed(Some(grant), Some(enrollment), members, message_seq, now) => {
                DeliveryGrant::Allowed(grant.clone().view(enrollment, now))
            }
            _ => DeliveryGrant::Denied,
        })
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
        if role == Role::Revoked && p.role != Role::Revoked {
            p.grant_generation = p.grant_generation.checked_add(1).ok_or(Error::Invalid("grant_generation_exhausted"))?;
        }
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
        let mut target = target.normalize();
        if !validate_participant_change(&org, actor, members, &target, create)? {
            return Ok(());
        }
        let revoked = (target.role == Role::Revoked).then(|| target.principal.clone());
        if let Some(existing) = members.iter_mut().find(|p| p.principal == target.principal) {
            target.grant_generation = if target.role == Role::Revoked {
                existing.grant_generation.checked_add(1).ok_or(Error::Invalid("grant_generation_exhausted"))?
            } else { existing.grant_generation };
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
        if let Some(fence) = req.grant_fence.as_ref() {
            // Checked under the same lock as the commit, before idempotent replay.
            let (grant, enrollment, members) =
                g.grant_parts(fence.grant_id).ok_or(Error::Forbidden("grant_operation_denied"))?;
            check_grant_access(sender, thread_id, grant, enrollment, members, fence, GrantOperation::Publish)?;
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
        if let Some(generation) = req.expected_grant_generation {
            if !g.participants[&thread_id].iter().any(|p| p.principal == *sender && p.grant_generation == generation) {
                return Err(Error::Forbidden("stale_grant_generation"));
            }
        }
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
