//! Postgres qualification for enrollment and grant authority.
//!
//! Each test creates and drops its own database on the server named by
//! `DATABASE_URL` (which must allow CREATE DATABASE). Disposable servers only.
//!
//! ```bash
//! DATABASE_URL=postgres://postgres:...@127.0.0.1:<port>/postgres \
//!   cargo test --locked -p mq-server --test postgres_grants -- --ignored --test-threads=1
//! ```

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use mq_core::*;
use mq_server::postgres::PostgresStore;
use sqlx::Connection;

struct TestDb {
    admin_url: String,
    name: String,
    url: String,
}

impl TestDb {
    async fn create() -> Self {
        let admin_url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let name = format!("mq_grants_{}", uuid::Uuid::new_v4().simple());
        let (base, _) = admin_url.rsplit_once('/').expect("database URL path");
        let url = format!("{base}/{name}");
        let mut admin = sqlx::PgConnection::connect(&admin_url).await.expect("admin connect");
        sqlx::query(&format!("CREATE DATABASE {name}")).execute(&mut admin).await.expect("create database");
        Self { admin_url, name, url }
    }

    async fn drop_db(self) {
        let mut admin = sqlx::PgConnection::connect(&self.admin_url).await.expect("admin connect");
        sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", self.name)).execute(&mut admin).await.expect("drop database");
    }
}

fn human(org: &str, id: &str) -> Principal {
    Principal { kind: PrincipalKind::Human, id: id.into(), org_id: org.into() }
}

async fn fabric(url: &str, clock: &Arc<Mutex<DateTime<Utc>>>) -> Fabric {
    let source = clock.clone();
    Fabric::from_store(Arc::new(PostgresStore::connect(url).await.expect("connect+migrate")))
        .with_clock(Arc::new(move || *source.lock().unwrap()))
}

fn fence(grant: &Grant, ops: &[GrantOperation]) -> GrantFence {
    GrantFence {
        grant_id: grant.grant_id, thread_id: grant.thread_id, enrollment_id: grant.enrollment_id,
        operations: ops.to_vec(), generation: grant.generation, incarnation: grant.incarnation, at: Utc::now(),
    }
}

const R: &[GrantOperation] = &[GrantOperation::Read];
const RP: &[GrantOperation] = &[GrantOperation::Read, GrantOperation::Publish];

#[tokio::test]
#[ignore = "requires disposable DATABASE_URL Postgres with CREATE DATABASE"]
async fn postgres_grant_authority_lifecycle() {
    let db = TestDb::create().await;
    let clock = Arc::new(Mutex::new(Utc::now()));
    let mq = fabric(&db.url, &clock).await;
    let owner = human("org", "owner");
    let member = human("org", "member");
    let thread = mq.create_thread(&owner, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
        participants: vec![Participant::new(owner.clone(), Role::Owner), Participant::new(member.clone(), Role::Member)],
        idempotency_key: None,
    }).await.unwrap().thread_id;
    for body in ["one", "two"] {
        mq.publish(&owner, thread, PublishMessage { body: body.into(), ..Default::default() }).await.unwrap();
    }

    // Enrollment: single-statement incarnation advance, owner scoped.
    let req = EnrollDevice { device_id: "dev".into(), session_id: "s1".into(), label: None };
    let enrollment = mq.enroll(&owner, req.clone()).await.unwrap();
    assert_eq!(enrollment.incarnation, 1);
    let theirs = mq.enroll(&member, req.clone()).await.unwrap();
    assert_ne!(theirs.enrollment_id, enrollment.enrollment_id);
    assert_eq!(mq.get_enrollment(&member, enrollment.enrollment_id).await, Err(Error::NotFound("enrollment")));

    // Creation authority and cross-account refusal.
    let create = |e: &Enrollment, ops: &[GrantOperation]| CreateGrant {
        thread_id: thread, enrollment_id: e.enrollment_id, operations: ops.to_vec(), ttl_seconds: 3600, history_after_seq: None,
    };
    assert_eq!(mq.create_grant(&member, create(&theirs, R)).await, Err(Error::Forbidden("invite_required")));
    assert_eq!(mq.create_grant(&owner, create(&theirs, R)).await, Err(Error::NotFound("enrollment")));
    let foreign = human("org-2", "owner");
    let foreign_enrollment = mq.enroll(&foreign, req.clone()).await.unwrap();
    assert_eq!(mq.create_grant(&foreign, create(&foreign_enrollment, R)).await, Err(Error::NotFound("thread")));
    let grant = mq.create_grant(&owner, create(&enrollment, R)).await.unwrap();
    assert_eq!((grant.history_after_seq, grant.generation, grant.incarnation), (2, 0, 1));
    assert_eq!(mq.create_grant(&owner, create(&enrollment, R)).await, Err(Error::Conflict("grant_exists")));
    assert!(mq.store().list_participants(thread).await.unwrap().iter().any(|p| p.principal == grant.principal && p.role == Role::Observer));
    assert_eq!(mq.get_grant(&member, grant.grant_id).await, Err(Error::NotFound("grant")));
    assert_eq!(mq.list_grants(&owner, &GrantFilter { enrollment_id: Some(enrollment.enrollment_id), thread_id: None }).await.unwrap().len(), 1);

    // History floor and explicit skip.
    mq.publish(&owner, thread, PublishMessage { body: "three".into(), ..Default::default() }).await.unwrap();
    let page = mq.read_history(&grant.principal, thread, HistoryAuthority::Grant(fence(&grant, R)), 0, 50).await.unwrap();
    assert_eq!(page.skipped.map(|s| (s.after_seq, s.through_seq)), Some((0, 2)));
    assert_eq!(page.messages.iter().map(|m| m.seq).collect::<Vec<_>>(), vec![3]);
    assert_eq!(mq.read_granted_messages(&grant.principal, thread, fence(&grant, R), 0, 10).await, Err(Error::Conflict("history_cursor_before_floor")));
    let job = mq.claim_delivery_jobs(10).await.unwrap().into_iter().find(|j| j.recipient == grant.principal).expect("job for device");
    assert!(matches!(mq.delivery_grant(&job, 3).await.unwrap(), DeliveryGrant::Allowed(_)));
    assert_eq!(mq.delivery_grant(&job, 2).await.unwrap(), DeliveryGrant::Denied);
    mq.settle_delivery_job(job.job_id, job.attempts, DeliveryStatus::Pending).await.unwrap();

    // Operation enforcement and incarnation fencing at the atomic publish boundary.
    let writer_enrollment = mq.enroll(&owner, EnrollDevice { device_id: "writer".into(), ..req.clone() }).await.unwrap();
    let rw = mq.create_grant(&owner, create(&writer_enrollment, RP)).await.unwrap();
    let publish = |g: &Grant, ops: &[GrantOperation], body: &str| PublishMessage { body: body.into(), grant_fence: Some(fence(g, ops)), ..Default::default() };
    assert_eq!(mq.publish(&grant.principal, thread, publish(&grant, &[GrantOperation::Publish], "x")).await.map(|_| ()), Err(Error::Forbidden("grant_operation_denied")));
    mq.publish(&rw.principal, thread, publish(&rw, RP, "from device")).await.unwrap();
    let old_process = fence(&rw, RP);
    assert_eq!(mq.enroll(&owner, EnrollDevice { device_id: "writer".into(), ..req.clone() }).await.unwrap().incarnation, 2);
    let stale = PublishMessage { body: "stale".into(), grant_fence: Some(old_process), ..Default::default() };
    assert_eq!(mq.publish(&rw.principal, thread, stale).await.map(|_| ()), Err(Error::Forbidden("grant_incarnation_fenced")));
    assert!(mq.read_messages(&owner, thread, 0, 50).await.unwrap().iter().all(|m| m.body != "stale"));
    let stale_issue = GrantIssuanceRequest { enrollment_id: writer_enrollment.enrollment_id, incarnation: 1 };
    assert_eq!(mq.grant_issuance(&owner, rw.grant_id, stale_issue).await.map(|_| ()), Err(Error::Forbidden("grant_incarnation_fenced")));

    // Revoke dead-letters queued delivery, refuses renew/issuance; restore keeps generation.
    let revoked = mq.revoke_grant(&owner, grant.grant_id).await.unwrap();
    assert_eq!((revoked.status, revoked.generation), (GrantStatus::Revoked, 1));
    assert_eq!(mq.revoke_grant(&owner, grant.grant_id).await.unwrap().generation, 1);
    *clock.lock().unwrap() += Duration::seconds(5);
    assert!(mq.claim_delivery_jobs(10).await.unwrap().iter().all(|j| j.recipient != grant.principal));
    assert_eq!(mq.read_history(&grant.principal, thread, HistoryAuthority::Grant(fence(&grant, R)), 2, 10).await.map(|_| ()), Err(Error::Forbidden("grant_revoked")));
    assert_eq!(mq.renew_grant(&owner, grant.grant_id, 3600).await, Err(Error::Forbidden("grant_revoked")));
    let issue = GrantIssuanceRequest { enrollment_id: enrollment.enrollment_id, incarnation: 1 };
    assert_eq!(mq.grant_issuance(&owner, grant.grant_id, issue.clone()).await.map(|_| ()), Err(Error::Forbidden("grant_revoked")));
    let restored = mq.restore_grant(&owner, grant.grant_id).await.unwrap();
    assert_eq!((restored.status, restored.generation), (GrantStatus::Active, 1));
    assert_eq!(mq.read_history(&grant.principal, thread, HistoryAuthority::Grant(fence(&grant, R)), 2, 10).await.map(|_| ()), Err(Error::Forbidden("grant_generation_stale")));
    let fresh = mq.grant_issuance(&owner, grant.grant_id, issue.clone()).await.unwrap().grant;
    assert!(mq.read_history(&fresh.principal, thread, HistoryAuthority::Grant(fence(&fresh, R)), 2, 10).await.is_ok());

    // Expiry by server clock, then renew.
    *clock.lock().unwrap() += Duration::seconds(3601);
    assert_eq!(mq.read_history(&fresh.principal, thread, HistoryAuthority::Grant(fence(&fresh, R)), 2, 10).await.map(|_| ()), Err(Error::Forbidden("grant_expired")));
    assert_eq!(mq.grant_issuance(&owner, grant.grant_id, issue.clone()).await.map(|_| ()), Err(Error::Forbidden("grant_expired")));
    assert_eq!(mq.renew_grant(&owner, grant.grant_id, 600).await.unwrap().state, GrantState::Active);

    // A fresh store (process restart) reads the persisted authority.
    let recovered = fabric(&db.url, &clock).await;
    let reloaded = recovered.get_grant(&owner, grant.grant_id).await.unwrap();
    assert_eq!((reloaded.status, reloaded.generation, reloaded.state), (GrantStatus::Active, 1, GrantState::Active));
    assert!(recovered.read_history(&fresh.principal, thread, HistoryAuthority::Grant(fence(&fresh, R)), 2, 10).await.is_ok());
    assert_eq!(recovered.enroll(&owner, req).await.unwrap().incarnation, 2);
    drop((mq, recovered));
    db.drop_db().await;
}

#[tokio::test]
#[ignore = "requires disposable DATABASE_URL Postgres with CREATE DATABASE"]
async fn postgres_enrollment_revocation_is_atomic_and_persistent() {
    let db = TestDb::create().await;
    let clock = Arc::new(Mutex::new(Utc::now()));
    let mq = fabric(&db.url, &clock).await;
    let owner = human("org", "owner");
    let new_thread = || CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
        participants: vec![Participant::new(owner.clone(), Role::Owner)], idempotency_key: None,
    };
    let (t1, t2, t3) = (
        mq.create_thread(&owner, new_thread()).await.unwrap().thread_id,
        mq.create_thread(&owner, new_thread()).await.unwrap().thread_id,
        mq.create_thread(&owner, new_thread()).await.unwrap().thread_id,
    );
    let req = EnrollDevice { device_id: "dev".into(), session_id: "s1".into(), label: None };
    let enrollment = mq.enroll(&owner, req.clone()).await.unwrap();
    let create = |thread, e: &Enrollment, ops: &[GrantOperation]| CreateGrant {
        thread_id: thread, enrollment_id: e.enrollment_id, operations: ops.to_vec(), ttl_seconds: 3600, history_after_seq: None,
    };
    let g1 = mq.create_grant(&owner, create(t1, &enrollment, RP)).await.unwrap();
    let g2 = mq.create_grant(&owner, create(t2, &enrollment, R)).await.unwrap();
    let other = mq.enroll(&owner, EnrollDevice { device_id: "other".into(), ..req.clone() }).await.unwrap();
    let kept = mq.create_grant(&owner, create(t1, &other, R)).await.unwrap();
    mq.publish(&owner, t1, PublishMessage { body: "leased".into(), ..Default::default() }).await.unwrap();
    let leased = mq.claim_delivery_jobs(10).await.unwrap().into_iter().find(|j| j.recipient == g1.principal).expect("leased job");
    mq.publish(&owner, t2, PublishMessage { body: "queued".into(), ..Default::default() }).await.unwrap();

    assert_eq!(mq.revoke_enrollment(&human("org", "member"), enrollment.enrollment_id).await, Err(Error::NotFound("enrollment")));
    let revoked = mq.revoke_enrollment(&owner, enrollment.enrollment_id).await.unwrap();
    assert!(revoked.revoked_at.is_some());
    assert_eq!(mq.revoke_enrollment(&owner, enrollment.enrollment_id).await.unwrap().revoked_at, revoked.revoked_at);

    for grant in [&g1, &g2] {
        let fence_read = mq.read_history(&grant.principal, grant.thread_id, HistoryAuthority::Grant(fence(grant, R)), 0, 10).await;
        assert_eq!(fence_read.map(|_| ()), Err(Error::Forbidden("enrollment_revoked")));
        let current = mq.get_grant(&owner, grant.grant_id).await.unwrap();
        assert_eq!((current.status, current.generation), (GrantStatus::Revoked, 1));
        assert_eq!(mq.restore_grant(&owner, grant.grant_id).await, Err(Error::Forbidden("enrollment_revoked")));
    }
    let issue = GrantIssuanceRequest { enrollment_id: enrollment.enrollment_id, incarnation: 1 };
    assert_eq!(mq.grant_issuance(&owner, g1.grant_id, issue).await.map(|_| ()), Err(Error::Forbidden("enrollment_revoked")));
    assert_eq!(mq.enroll(&owner, req.clone()).await, Err(Error::Forbidden("enrollment_revoked")));
    assert_eq!(mq.create_grant(&owner, create(t3, &enrollment, R)).await, Err(Error::Forbidden("enrollment_revoked")));
    let stale = PublishMessage { body: "after sign-out".into(), grant_fence: Some(fence(&g1, RP)), ..Default::default() };
    assert_eq!(mq.publish(&g1.principal, t1, stale).await.map(|_| ()), Err(Error::Forbidden("enrollment_revoked")));
    // Leased + queued jobs dead-lettered in the same transaction.
    let statuses: Vec<String> = sqlx::query_scalar("SELECT status FROM mq_delivery_jobs WHERE recipient_id=$1")
        .bind(&g1.principal.id).fetch_all(mq_pool(&db.url).await.as_ref().unwrap()).await.unwrap();
    assert_eq!(statuses, vec!["dead_letter".to_string(), "dead_letter".to_string()]);
    assert!(mq.settle_delivery_job(leased.job_id, leased.attempts, DeliveryStatus::Delivered).await.is_err());
    assert_eq!(mq.delivery_grant(&leased, 1).await.unwrap(), DeliveryGrant::Denied);
    // Unrelated enrollment keeps working; restart keeps the sign-out.
    assert!(mq.read_history(&kept.principal, t1, HistoryAuthority::Grant(fence(&kept, R)), kept.history_after_seq, 10).await.is_ok());
    let recovered = fabric(&db.url, &clock).await;
    assert!(recovered.get_enrollment(&owner, enrollment.enrollment_id).await.unwrap().revoked_at.is_some());
    assert_eq!(recovered.enroll(&owner, req).await, Err(Error::Forbidden("enrollment_revoked")));
    drop((mq, recovered));
    db.drop_db().await;
}

async fn mq_pool(url: &str) -> Option<sqlx::PgPool> {
    sqlx::PgPool::connect(url).await.ok()
}

#[tokio::test]
#[ignore = "requires disposable DATABASE_URL Postgres with CREATE DATABASE"]
async fn postgres_grant_schema_refuses_forged_rows() {
    let db = TestDb::create().await;
    let clock = Arc::new(Mutex::new(Utc::now()));
    let mq = fabric(&db.url, &clock).await;
    let owner = human("org", "owner");
    let thread = mq.create_thread(&owner, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
        participants: vec![Participant::new(owner.clone(), Role::Owner)], idempotency_key: None,
    }).await.unwrap().thread_id;
    let enrollment = mq.enroll(&owner, EnrollDevice { device_id: "dev".into(), session_id: "s".into(), label: None }).await.unwrap();
    let mut conn = sqlx::PgConnection::connect(&db.url).await.unwrap();
    let insert = |principal_id: String, org: &str, operations: Vec<&str>| {
        let (org, operations) = (org.to_string(), operations.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        (principal_id, org, operations)
    };
    for (principal_id, org, operations) in [
        insert("enrollment:someone-else".into(), "org", vec!["read"]),
        insert(format!("enrollment:{}", enrollment.enrollment_id), "org-2", vec!["read"]),
        insert(format!("enrollment:{}", enrollment.enrollment_id), "org", vec!["invite"]),
        insert(format!("enrollment:{}", enrollment.enrollment_id), "org", vec![]),
    ] {
        let result = sqlx::query("INSERT INTO mq_grants (grant_id, org_id, thread_id, enrollment_id, principal_kind, principal_id, operations, history_after_seq, expires_at, status, granted_by_kind, granted_by_id, created_at, updated_at) VALUES ($1,$2,$3,$4,'actor',$5,$6,0,now(),'active','human','owner',now(),now())")
            .bind(uuid::Uuid::new_v4()).bind(&org).bind(thread.0).bind(enrollment.enrollment_id).bind(&principal_id).bind(&operations)
            .execute(&mut conn).await;
        assert!(result.is_err(), "forged grant row accepted: {principal_id} {org} {operations:?}");
    }
    let forged_owner = sqlx::query("INSERT INTO mq_enrollments (enrollment_id, org_id, owner_kind, owner_id, device_id, session_id, incarnation, created_at, updated_at) VALUES ($1,'org','actor','bot','d','s',1,now(),now())")
        .bind(uuid::Uuid::new_v4()).execute(&mut conn).await;
    assert!(forged_owner.is_err());
    drop(conn);
    drop(mq);
    db.drop_db().await;
}
