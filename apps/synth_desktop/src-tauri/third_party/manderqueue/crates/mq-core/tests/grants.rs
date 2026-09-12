//! Enrollment and grant authority over the memory store (docs/WORKSHOP_GRANT_CONTRACT.md).

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use mq_core::*;

fn human(org: &str, id: &str) -> Principal {
    Principal { kind: PrincipalKind::Human, id: id.into(), org_id: org.into() }
}

struct Fixture {
    mq: Fabric,
    clock: Arc<Mutex<DateTime<Utc>>>,
    owner: Principal,
    thread: ThreadId,
}

impl Fixture {
    fn advance(&self, seconds: i64) {
        *self.clock.lock().unwrap() += Duration::seconds(seconds);
    }
}

async fn fixture_with(store: Arc<dyn Store>) -> Fixture {
    let clock = Arc::new(Mutex::new(Utc::now()));
    let source = clock.clone();
    let mq = Fabric::from_store(store).with_clock(Arc::new(move || *source.lock().unwrap()));
    let owner = human("org", "owner");
    let thread = mq.create_thread(&owner, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
        participants: vec![Participant::new(owner.clone(), Role::Owner), Participant::new(human("org", "member"), Role::Member)],
        idempotency_key: None,
    }).await.unwrap().thread_id;
    Fixture { mq, clock, owner, thread }
}

async fn fixture() -> Fixture {
    fixture_with(Arc::new(MemoryStore::default())).await
}

fn enroll_req(device: &str) -> EnrollDevice {
    EnrollDevice { device_id: device.into(), session_id: "session-1".into(), label: None }
}

fn create(thread: ThreadId, enrollment: &Enrollment, ops: &[GrantOperation]) -> CreateGrant {
    CreateGrant { thread_id: thread, enrollment_id: enrollment.enrollment_id, operations: ops.to_vec(), ttl_seconds: 3600, history_after_seq: None }
}

fn fence(grant: &Grant, ops: &[GrantOperation]) -> GrantFence {
    GrantFence {
        grant_id: grant.grant_id, thread_id: grant.thread_id, enrollment_id: grant.enrollment_id,
        operations: ops.to_vec(), generation: grant.generation, incarnation: grant.incarnation, at: Utc::now(),
    }
}

async fn publish(f: &Fixture, body: &str) -> Message {
    f.mq.publish(&f.owner, f.thread, PublishMessage { body: body.into(), ..Default::default() }).await.unwrap()
}

async fn read(f: &Fixture, grant: &Grant, fence: GrantFence, after: u64) -> Result<HistoryPage> {
    f.mq.read_history(&grant.principal, f.thread, HistoryAuthority::Grant(fence), after, 50).await
}

const R: &[GrantOperation] = &[GrantOperation::Read];
const RP: &[GrantOperation] = &[GrantOperation::Read, GrantOperation::Publish];

#[tokio::test]
async fn enrollment_incarnation_advances_and_is_owner_scoped() {
    let f = fixture().await;
    let first = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let second = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    assert_eq!((first.incarnation, second.incarnation), (1, 2));
    assert_eq!(first.enrollment_id, second.enrollment_id);
    assert_eq!(second.principal, grants::enrollment_principal("org", first.enrollment_id));
    // Another account with the same device/session gets an independent record.
    let other = human("org", "member");
    let theirs = f.mq.enroll(&other, enroll_req("dev")).await.unwrap();
    assert_ne!(theirs.enrollment_id, first.enrollment_id);
    assert_eq!(theirs.incarnation, 1);
    assert_eq!(f.mq.get_enrollment(&other, first.enrollment_id).await, Err(Error::NotFound("enrollment")));
    assert_eq!(f.mq.get_enrollment(&human("org-2", "owner"), first.enrollment_id).await, Err(Error::NotFound("enrollment")));
    assert_eq!(f.mq.list_enrollments(&f.owner).await.unwrap().len(), 1);
    let actor = Principal { kind: PrincipalKind::Actor, ..f.owner.clone() };
    assert_eq!(f.mq.enroll(&actor, enroll_req("dev")).await, Err(Error::Forbidden("enrollment_owner_must_be_human")));
    assert_eq!(f.mq.enroll(&f.owner, enroll_req("bad device")).await, Err(Error::Invalid("invalid_device_identity")));
}

#[tokio::test]
async fn grant_creation_requires_owned_enrollment_invite_and_same_org() {
    let f = fixture().await;
    let mine = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let member = human("org", "member");
    let theirs = f.mq.enroll(&member, enroll_req("dev")).await.unwrap();
    // Member lacks invite; owner cannot grant to another account's device.
    assert_eq!(f.mq.create_grant(&member, create(f.thread, &theirs, R)).await, Err(Error::Forbidden("invite_required")));
    assert_eq!(f.mq.create_grant(&f.owner, create(f.thread, &theirs, R)).await, Err(Error::NotFound("enrollment")));
    // Cross-org caller sees neither thread nor enrollment.
    let foreign = human("org-2", "owner");
    let foreign_enrollment = f.mq.enroll(&foreign, enroll_req("dev")).await.unwrap();
    assert_eq!(f.mq.create_grant(&foreign, create(f.thread, &foreign_enrollment, R)).await, Err(Error::NotFound("thread")));
    // Validation.
    let mut bad = create(f.thread, &mine, &[]);
    assert_eq!(f.mq.create_grant(&f.owner, bad.clone()).await, Err(Error::Invalid("invalid_operations")));
    bad.operations = R.to_vec();
    bad.ttl_seconds = 59;
    assert_eq!(f.mq.create_grant(&f.owner, bad.clone()).await, Err(Error::Invalid("invalid_ttl")));
    bad.ttl_seconds = 60;
    bad.history_after_seq = Some(1);
    assert_eq!(f.mq.create_grant(&f.owner, bad).await, Err(Error::Invalid("invalid_history_bound")));

    let grant = f.mq.create_grant(&f.owner, create(f.thread, &mine, R)).await.unwrap();
    assert_eq!(grant.principal, mine.principal);
    assert_eq!((grant.generation, grant.incarnation, grant.state), (0, 1, GrantState::Active));
    let members = f.mq.store().list_participants(f.thread).await.unwrap();
    assert_eq!(members.iter().find(|p| p.principal == mine.principal).unwrap().role, Role::Observer);
    assert_eq!(f.mq.create_grant(&f.owner, create(f.thread, &mine, R)).await, Err(Error::Conflict("grant_exists")));
    // Visibility: owner yes; plain member and other org no.
    assert!(f.mq.get_grant(&f.owner, grant.grant_id).await.is_ok());
    assert_eq!(f.mq.get_grant(&member, grant.grant_id).await, Err(Error::NotFound("grant")));
    assert_eq!(f.mq.get_grant(&foreign, grant.grant_id).await, Err(Error::NotFound("grant")));
    assert!(f.mq.list_grants(&member, &GrantFilter::default()).await.unwrap().is_empty());
    assert_eq!(f.mq.list_grants(&f.owner, &GrantFilter { thread_id: Some(f.thread), enrollment_id: None }).await.unwrap().len(), 1);
    // Member cannot revoke/restore/renew a grant it cannot see.
    assert_eq!(f.mq.revoke_grant(&member, grant.grant_id).await, Err(Error::NotFound("grant")));
    assert_eq!(f.mq.grant_issuance(&member, grant.grant_id, GrantIssuanceRequest { enrollment_id: mine.enrollment_id, incarnation: 1 }).await.map(|_| ()), Err(Error::NotFound("grant")));
}

#[tokio::test]
async fn granted_history_is_bounded_and_skips_explicitly() {
    let f = fixture().await;
    for body in ["before-1", "before-2", "before-3"] {
        publish(&f, body).await;
    }
    let enrollment = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let grant = f.mq.create_grant(&f.owner, create(f.thread, &enrollment, R)).await.unwrap();
    assert_eq!(grant.history_after_seq, 3);
    publish(&f, "after-4").await;
    publish(&f, "after-5").await;

    let page = read(&f, &grant, fence(&grant, R), 0).await.unwrap();
    assert_eq!(page.skipped, Some(HistorySkip { after_seq: 0, through_seq: 3, reason: "before_grant_history".into() }));
    assert_eq!(page.messages.iter().map(|m| m.seq).collect::<Vec<_>>(), vec![4, 5]);
    assert_eq!((page.effective_after_seq, page.next_after_seq, page.has_more), (3, 5, false));
    let page = f.mq.read_history(&grant.principal, f.thread, HistoryAuthority::Grant(fence(&grant, R)), 3, 1).await.unwrap();
    assert!(page.skipped.is_none());
    assert_eq!((page.next_after_seq, page.has_more), (4, true));
    let page = read(&f, &grant, fence(&grant, R), 5).await.unwrap();
    assert!(page.messages.is_empty() && page.next_after_seq == 5 && !page.has_more);
    // The legacy array endpoint refuses rather than silently filtering.
    assert_eq!(f.mq.read_granted_messages(&grant.principal, f.thread, fence(&grant, R), 0, 50).await, Err(Error::Conflict("history_cursor_before_floor")));
    assert_eq!(f.mq.read_granted_messages(&grant.principal, f.thread, fence(&grant, R), 3, 50).await.unwrap().len(), 2);
    // An explicit floor of zero grants the whole history.
    let second = f.mq.enroll(&f.owner, enroll_req("dev-2")).await.unwrap();
    let full = f.mq.create_grant(&f.owner, CreateGrant { history_after_seq: Some(0), ..create(f.thread, &second, R) }).await.unwrap();
    let page = read(&f, &full, fence(&full, R), 0).await.unwrap();
    assert!(page.skipped.is_none());
    assert_eq!(page.messages.len(), 5);
    // Membership credentials see full history with floor 0.
    let page = f.mq.read_history(&f.owner, f.thread, HistoryAuthority::Membership, 0, 2).await.unwrap();
    assert_eq!((page.history_after_seq, page.next_after_seq, page.has_more), (0, 2, true));
}

#[tokio::test]
async fn grant_operations_are_enforced_on_publish() {
    let f = fixture().await;
    let reader = f.mq.enroll(&f.owner, enroll_req("reader")).await.unwrap();
    let read_only = f.mq.create_grant(&f.owner, create(f.thread, &reader, R)).await.unwrap();
    let publish_as = |grant: &Grant, ops: &[GrantOperation]| PublishMessage {
        body: "from device".into(), grant_fence: Some(fence(grant, ops)), ..Default::default()
    };
    // Even a credential claiming publish cannot exceed the stored grant.
    assert_eq!(f.mq.publish(&read_only.principal, f.thread, publish_as(&read_only, &[GrantOperation::Publish])).await,
        Err(Error::Forbidden("grant_operation_denied")));
    let writer = f.mq.enroll(&f.owner, enroll_req("writer")).await.unwrap();
    let rw = f.mq.create_grant(&f.owner, create(f.thread, &writer, RP)).await.unwrap();
    let message = f.mq.publish(&rw.principal, f.thread, publish_as(&rw, RP)).await.unwrap();
    assert_eq!(message.sender, rw.principal);
    // A grant for one thread cannot publish in another.
    let other = f.mq.create_thread(&f.owner, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
        participants: vec![Participant::new(f.owner.clone(), Role::Owner)], idempotency_key: None,
    }).await.unwrap().thread_id;
    assert!(f.mq.publish(&rw.principal, other, publish_as(&rw, RP)).await.is_err());
}

#[tokio::test]
async fn revoke_restore_generation_and_offline_renew_refusal() {
    let f = fixture().await;
    let enrollment = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let grant = f.mq.create_grant(&f.owner, create(f.thread, &enrollment, R)).await.unwrap();
    let old = fence(&grant, R);
    assert!(read(&f, &grant, old.clone(), grant.history_after_seq).await.is_ok());
    let revoked = f.mq.revoke_grant(&f.owner, grant.grant_id).await.unwrap();
    assert_eq!((revoked.status, revoked.generation, revoked.state), (GrantStatus::Revoked, 1, GrantState::Revoked));
    assert_eq!(f.mq.revoke_grant(&f.owner, grant.grant_id).await.unwrap().generation, 1, "revoke is idempotent");
    assert_eq!(read(&f, &grant, old.clone(), 0).await.map(|_| ()), Err(Error::Forbidden("grant_revoked")));
    // Offline device returning after revoke: neither renew nor issuance restores it.
    assert_eq!(f.mq.renew_grant(&f.owner, grant.grant_id, 3600).await, Err(Error::Forbidden("grant_revoked")));
    let issue = GrantIssuanceRequest { enrollment_id: enrollment.enrollment_id, incarnation: 1 };
    assert_eq!(f.mq.grant_issuance(&f.owner, grant.grant_id, issue.clone()).await.map(|_| ()), Err(Error::Forbidden("grant_revoked")));
    // Restore keeps the advanced generation: the pre-revoke credential stays dead.
    let restored = f.mq.restore_grant(&f.owner, grant.grant_id).await.unwrap();
    assert_eq!((restored.status, restored.generation), (GrantStatus::Active, 1));
    assert_eq!(read(&f, &grant, old, 0).await.map(|_| ()), Err(Error::Forbidden("grant_generation_stale")));
    let fresh = f.mq.grant_issuance(&f.owner, grant.grant_id, issue).await.unwrap();
    assert_eq!(fresh.not_after, fresh.grant.expires_at);
    assert!(read(&f, &fresh.grant, fence(&fresh.grant, R), 0).await.is_ok());
    // Only an invite holder may restore/renew.
    let member = human("org", "member");
    assert_eq!(f.mq.renew_grant(&member, grant.grant_id, 3600).await, Err(Error::NotFound("grant")));
}

#[tokio::test]
async fn expiry_is_enforced_and_renew_extends() {
    let f = fixture().await;
    let enrollment = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let grant = f.mq.create_grant(&f.owner, CreateGrant { ttl_seconds: 60, ..create(f.thread, &enrollment, R) }).await.unwrap();
    let issue = GrantIssuanceRequest { enrollment_id: enrollment.enrollment_id, incarnation: 1 };
    f.advance(61);
    assert_eq!(read(&f, &grant, fence(&grant, R), 0).await.map(|_| ()), Err(Error::Forbidden("grant_expired")));
    assert_eq!(f.mq.grant_issuance(&f.owner, grant.grant_id, issue.clone()).await.map(|_| ()), Err(Error::Forbidden("grant_expired")));
    assert_eq!(f.mq.get_grant(&f.owner, grant.grant_id).await.unwrap().state, GrantState::Expired);
    // An expired revoked grant cannot be restored; renew after revoke refuses.
    f.mq.revoke_grant(&f.owner, grant.grant_id).await.unwrap();
    assert_eq!(f.mq.restore_grant(&f.owner, grant.grant_id).await, Err(Error::Forbidden("grant_expired")));
    let second = f.mq.enroll(&f.owner, enroll_req("dev-2")).await.unwrap();
    let other = f.mq.create_grant(&f.owner, CreateGrant { ttl_seconds: 60, ..create(f.thread, &second, R) }).await.unwrap();
    f.advance(61);
    let renewed = f.mq.renew_grant(&f.owner, other.grant_id, 120).await.unwrap();
    assert_eq!((renewed.state, renewed.generation), (GrantState::Active, 0));
    assert!(read(&f, &renewed, fence(&renewed, R), 0).await.is_ok());
}

#[tokio::test]
async fn new_incarnation_fences_old_process() {
    let f = fixture().await;
    let enrollment = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let grant = f.mq.create_grant(&f.owner, create(f.thread, &enrollment, RP)).await.unwrap();
    let old = fence(&grant, RP);
    assert!(read(&f, &grant, old.clone(), 0).await.is_ok());
    let next = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    assert_eq!(next.incarnation, 2);
    assert_eq!(read(&f, &grant, old.clone(), 0).await.map(|_| ()), Err(Error::Forbidden("grant_incarnation_fenced")));
    let stale_publish = PublishMessage { body: "old process".into(), grant_fence: Some(old), ..Default::default() };
    assert_eq!(f.mq.publish(&grant.principal, f.thread, stale_publish).await.map(|_| ()), Err(Error::Forbidden("grant_incarnation_fenced")));
    let stale = GrantIssuanceRequest { enrollment_id: enrollment.enrollment_id, incarnation: 1 };
    assert_eq!(f.mq.grant_issuance(&f.owner, grant.grant_id, stale).await.map(|_| ()), Err(Error::Forbidden("grant_incarnation_fenced")));
    let current = f.mq.grant_issuance(&f.owner, grant.grant_id, GrantIssuanceRequest { enrollment_id: enrollment.enrollment_id, incarnation: 2 }).await.unwrap();
    assert_eq!(current.grant.incarnation, 2);
    assert!(read(&f, &current.grant, fence(&current.grant, RP), 0).await.is_ok());
    assert!(f.mq.read_messages(&f.owner, f.thread, 0, 10).await.unwrap().is_empty(), "stale publish left no row");
}

#[tokio::test]
async fn membership_revocation_blocks_grant_and_new_grants() {
    let f = fixture().await;
    let enrollment = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let grant = f.mq.create_grant(&f.owner, create(f.thread, &enrollment, R)).await.unwrap();
    f.mq.set_participant_role(&f.owner, f.thread, &grant.principal, Role::Revoked).await.unwrap();
    assert_eq!(read(&f, &grant, fence(&grant, R), 0).await.map(|_| ()), Err(Error::Forbidden("grant_membership_required")));
    f.mq.revoke_grant(&f.owner, grant.grant_id).await.unwrap();
    assert_eq!(f.mq.restore_grant(&f.owner, grant.grant_id).await, Err(Error::Forbidden("grant_membership_required")));
}

#[tokio::test]
async fn queued_delivery_requires_a_live_read_grant() {
    let f = fixture().await;
    let enrollment = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let grant = f.mq.create_grant(&f.owner, create(f.thread, &enrollment, R)).await.unwrap();
    let message = publish(&f, "to device").await;
    let jobs: Vec<_> = f.mq.claim_delivery_jobs(10).await.unwrap().into_iter().filter(|j| j.recipient == grant.principal).collect();
    assert_eq!(jobs.len(), 1);
    assert!(matches!(f.mq.delivery_grant(&jobs[0], message.seq).await.unwrap(), DeliveryGrant::Allowed(ref g) if g.generation == 0 && g.incarnation == 1));
    assert_eq!(f.mq.delivery_grant(&jobs[0], grant.history_after_seq).await.unwrap(), DeliveryGrant::Denied, "at or below the floor");
    // A pending job queued before revoke is dead-lettered by the revoke itself.
    f.mq.settle_delivery_job(jobs[0].job_id, jobs[0].attempts, DeliveryStatus::Pending).await.unwrap();
    f.mq.revoke_grant(&f.owner, grant.grant_id).await.unwrap();
    f.advance(600);
    assert!(f.mq.claim_delivery_jobs(10).await.unwrap().iter().all(|j| j.recipient != grant.principal));
    assert_eq!(f.mq.delivery_grant(&jobs[0], message.seq).await.unwrap(), DeliveryGrant::Denied);
    // New fan-out skips the revoked device; a directed send refuses.
    publish(&f, "after revoke").await;
    assert!(f.mq.claim_delivery_jobs(10).await.unwrap().iter().all(|j| j.recipient != grant.principal));
    let directed = PublishMessage { body: "directed".into(), recipients: vec![grant.principal.clone()], ..Default::default() };
    assert_eq!(f.mq.publish(&f.owner, f.thread, directed).await.map(|_| ()), Err(Error::Invalid("recipient_grant_inactive")));
    // Ordinary recipients are not governed by grants.
    let member_job = DeliveryJob { recipient: human("org", "member"), ..jobs[0].clone() };
    assert_eq!(f.mq.delivery_grant(&member_job, message.seq).await.unwrap(), DeliveryGrant::NotGoverned);
}

#[tokio::test]
async fn checkpoint_round_trips_grant_authority() {
    let store = MemoryStore::default();
    let f = fixture_with(Arc::new(store.clone())).await;
    assert_eq!(store.checkpoint().version, 2, "no grants keeps the older format");
    let enrollment = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let grant = f.mq.create_grant(&f.owner, create(f.thread, &enrollment, R)).await.unwrap();
    f.mq.revoke_grant(&f.owner, grant.grant_id).await.unwrap();
    f.mq.restore_grant(&f.owner, grant.grant_id).await.unwrap();
    let snapshot = store.checkpoint();
    assert_eq!((snapshot.version, snapshot.enrollments.len(), snapshot.grants.len()), (3, 1, 1));
    let restored = Fabric::from_store(Arc::new(MemoryStore::from_checkpoint(snapshot.clone()).unwrap()));
    let reloaded = restored.get_grant(&f.owner, grant.grant_id).await.unwrap();
    assert_eq!((reloaded.generation, reloaded.status), (1, GrantStatus::Active));
    assert_eq!(restored.enroll(&f.owner, enroll_req("dev")).await.unwrap().incarnation, 2);
    let mut tampered = snapshot.clone();
    tampered.grants[0].principal.id = "enrollment:someone-else".into();
    assert!(MemoryStore::from_checkpoint(tampered).is_err());
    let mut downgraded = snapshot;
    downgraded.version = 2;
    assert!(MemoryStore::from_checkpoint(downgraded).is_err());
}

#[tokio::test]
async fn enrollment_revocation_signs_out_the_device_everywhere() {
    let store = MemoryStore::default();
    let f = fixture_with(Arc::new(store.clone())).await;
    let enrollment = f.mq.enroll(&f.owner, enroll_req("dev")).await.unwrap();
    let other_thread = f.mq.create_thread(&f.owner, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
        participants: vec![Participant::new(f.owner.clone(), Role::Owner)], idempotency_key: None,
    }).await.unwrap().thread_id;
    let g1 = f.mq.create_grant(&f.owner, create(f.thread, &enrollment, RP)).await.unwrap();
    let g2 = f.mq.create_grant(&f.owner, create(other_thread, &enrollment, R)).await.unwrap();
    let unrelated = f.mq.enroll(&f.owner, enroll_req("other-device")).await.unwrap();
    let kept = f.mq.create_grant(&f.owner, create(f.thread, &unrelated, R)).await.unwrap();
    // One leased and one queued delivery for the signed-out device.
    publish(&f, "leased").await;
    let leased: Vec<_> = f.mq.claim_delivery_jobs(10).await.unwrap().into_iter().filter(|j| j.recipient == g1.principal).collect();
    assert_eq!(leased.len(), 1);
    f.mq.publish(&f.owner, other_thread, PublishMessage { body: "queued".into(), ..Default::default() }).await.unwrap();

    let member = human("org", "member");
    assert_eq!(f.mq.revoke_enrollment(&member, enrollment.enrollment_id).await, Err(Error::NotFound("enrollment")));
    assert_eq!(f.mq.revoke_enrollment(&human("org-2", "owner"), enrollment.enrollment_id).await, Err(Error::NotFound("enrollment")));
    let revoked = f.mq.revoke_enrollment(&f.owner, enrollment.enrollment_id).await.unwrap();
    assert!(revoked.revoked_at.is_some());
    assert_eq!(f.mq.revoke_enrollment(&f.owner, enrollment.enrollment_id).await.unwrap().revoked_at, revoked.revoked_at, "idempotent");

    // Every grant and every incarnation is refused.
    for grant in [&g1, &g2] {
        let on_own_thread = f.mq.read_history(&grant.principal, grant.thread_id, HistoryAuthority::Grant(fence(grant, R)), 0, 10).await;
        assert_eq!(on_own_thread.map(|_| ()), Err(Error::Forbidden("enrollment_revoked")));
        let current = f.mq.get_grant(&f.owner, grant.grant_id).await.unwrap();
        assert_eq!((current.status, current.state, current.generation), (GrantStatus::Revoked, GrantState::Revoked, 1));
        assert_eq!(f.mq.restore_grant(&f.owner, grant.grant_id).await, Err(Error::Forbidden("enrollment_revoked")));
        assert_eq!(f.mq.renew_grant(&f.owner, grant.grant_id, 600).await, Err(Error::Forbidden("enrollment_revoked")));
        let issue = GrantIssuanceRequest { enrollment_id: enrollment.enrollment_id, incarnation: 1 };
        assert_eq!(f.mq.grant_issuance(&f.owner, grant.grant_id, issue).await.map(|_| ()), Err(Error::Forbidden("enrollment_revoked")));
    }
    let stale_publish = PublishMessage { body: "after sign-out".into(), grant_fence: Some(fence(&g1, RP)), ..Default::default() };
    assert_eq!(f.mq.publish(&g1.principal, f.thread, stale_publish).await.map(|_| ()), Err(Error::Forbidden("enrollment_revoked")));
    assert_eq!(f.mq.enroll(&f.owner, enroll_req("dev")).await, Err(Error::Forbidden("enrollment_revoked")));
    let third = f.mq.create_thread(&f.owner, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
        participants: vec![Participant::new(f.owner.clone(), Role::Owner)], idempotency_key: None,
    }).await.unwrap().thread_id;
    assert_eq!(f.mq.create_grant(&f.owner, create(third, &enrollment, R)).await, Err(Error::Forbidden("enrollment_revoked")));

    // Queued and leased deliveries are dead-lettered; the late settle is refused.
    f.advance(600);
    let jobs = store.checkpoint().jobs;
    assert!(jobs.iter().filter(|j| j.recipient == g1.principal).all(|j| j.status == DeliveryStatus::DeadLetter));
    assert_eq!(jobs.iter().filter(|j| j.recipient == g1.principal).count(), 2);
    assert!(f.mq.settle_delivery_job(leased[0].job_id, leased[0].attempts, DeliveryStatus::Delivered).await.is_err());
    assert_eq!(f.mq.delivery_grant(&leased[0], 1).await.unwrap(), DeliveryGrant::Denied);

    // Other enrollments of the same owner are unaffected; a new session enrolls fresh.
    assert!(read(&f, &kept, fence(&kept, R), kept.history_after_seq).await.is_ok());
    let fresh = f.mq.enroll(&f.owner, EnrollDevice { session_id: "session-2".into(), ..enroll_req("dev") }).await.unwrap();
    assert_ne!(fresh.enrollment_id, enrollment.enrollment_id);
    // Checkpoints keep the sign-out.
    let reloaded = Fabric::from_store(Arc::new(MemoryStore::from_checkpoint(store.checkpoint()).unwrap()));
    assert!(reloaded.get_enrollment(&f.owner, enrollment.enrollment_id).await.unwrap().revoked_at.is_some());
}
