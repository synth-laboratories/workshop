//! Postgres-backed fabric tests. Requires DATABASE_URL (compose: postgres on :5433).
//!
//! ```bash
//! docker compose up -d postgres
//! DATABASE_URL=postgres://mq:mq@127.0.0.1:5433/manderqueue cargo test -p mq-server --test postgres_fabric -- --ignored
//! ```

use std::sync::Arc;

use mq_core::*;
use mq_server::postgres::PostgresStore;

fn human(org: &str, id: &str) -> Principal {
    Principal {
        kind: PrincipalKind::Human,
        id: id.into(),
        org_id: org.into(),
    }
}

fn async_intern(org: &str, id: &str) -> Principal {
    Principal {
        kind: PrincipalKind::InternAsync,
        id: id.into(),
        org_id: org.into(),
    }
}

#[tokio::test]
#[ignore = "requires disposable DATABASE_URL Postgres"]
async fn postgres_revocation_cancels_queued_and_leased_jobs() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let store = PostgresStore::connect(&url).await.expect("connect+migrate");
    let mq = Fabric::from_store(Arc::new(store));
    let org = format!("revocation-{}", uuid::Uuid::new_v4());
    let owner = human(&org, "owner");
    let target = async_intern(&org, "target");
    let thread = mq.create_thread(&owner, CreateThread {
        org_id: org.clone(), scope: ScopeBinding { kind: ScopeKind::Org, id: org.clone() },
        title: None, participants: vec![Participant::new(owner.clone(), Role::Owner), Participant::new(target.clone(), Role::Agent)], idempotency_key: None,
    }).await.unwrap().thread_id;
    for body in ["leased", "queued"] {
        mq.publish(&owner, thread, PublishMessage { body: body.into(), recipients: vec![target.clone()], ..Default::default() }).await.unwrap();
    }
    let claimed = mq.claim_delivery_jobs(1).await.unwrap();
    assert_eq!(claimed.len(), 1);
    mq.validate_grant_generation(&target, thread, 0).await.unwrap();
    mq.set_participant_role(&owner, thread, &target, Role::Revoked).await.unwrap();
    mq.set_participant_role(&owner, thread, &target, Role::Revoked).await.unwrap();
    assert!(mq.claim_delivery_jobs(10).await.unwrap().is_empty());
    assert!(mq.settle_delivery_job(claimed[0].job_id, claimed[0].attempts, DeliveryStatus::Delivered).await.is_err());
    assert!(mq.read_messages(&target, thread, 0, 10).await.is_err());
    assert!(mq.publish(&target, thread, PublishMessage { body: "stale".into(), ..Default::default() }).await.is_err());
    // Fresh store reload proves the role and cancelled jobs were persisted.
    let recovered = Fabric::from_store(Arc::new(PostgresStore::connect(&url).await.unwrap()));
    assert!(recovered.read_messages(&target, thread, 0, 10).await.is_err());
    recovered.set_participant_role(&owner, thread, &target, Role::Agent).await.unwrap();
    assert_eq!(recovered.read_messages(&target, thread, 0, 10).await.unwrap().len(), 2);
    assert!(recovered.claim_delivery_jobs(10).await.unwrap().is_empty());
    // Restoration retains the persisted generation; repeated revocation does
    // not increment it twice. An authorization captured before revocation must
    // also fail inside the append transaction, before messages or jobs exist.
    recovered.validate_grant_generation(&target, thread, 1).await.unwrap();
    assert!(recovered.validate_grant_generation(&target, thread, 0).await.is_err());
    let request = PublishMessage {
        body: "generation fenced".into(),
        recipients: vec![owner.clone()],
        idempotency_key: Some("generation-fenced".into()),
        expected_grant_generation: Some(0),
        ..Default::default()
    };
    assert!(matches!(recovered.publish(&target, thread, request.clone()).await,
        Err(Error::Forbidden(reason)) if reason == "stale_grant_generation"));
    assert_eq!(recovered.read_messages(&owner, thread, 0, 10).await.unwrap().len(), 2);
    assert!(recovered.claim_delivery_jobs(10).await.unwrap().is_empty());
    let current = PublishMessage { expected_grant_generation: Some(1), ..request.clone() };
    recovered.publish(&target, thread, current).await.unwrap();
    // A stale token cannot obtain an idempotent replay either.
    assert!(matches!(recovered.publish(&target, thread, request).await,
        Err(Error::Forbidden(reason)) if reason == "stale_grant_generation"));
    assert_eq!(recovered.read_messages(&owner, thread, 0, 10).await.unwrap().len(), 3);
    let jobs = recovered.claim_delivery_jobs(10).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].recipient, owner);
}

#[tokio::test]
#[ignore = "requires isolated disposable DATABASE_URL Postgres"]
async fn postgres_generation_upgrade_preserves_existing_rows() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    // Build the actual preceding schema, including SQLx's migration checksums.
    // Do not simulate an upgrade by changing a current-schema status marker.
    let mut preceding = sqlx::migrate!("./migrations");
    preceding.migrations.to_mut().retain(|migration| migration.version < 20260912120000);
    preceding.run(&pool).await.unwrap();
    let missing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema='public' AND table_name='mq_participants' AND column_name='grant_generation'"
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(missing, 0);
    let thread = ThreadId(uuid::Uuid::new_v4());
    let message = uuid::Uuid::new_v4();
    let job = uuid::Uuid::new_v4();
    let owner = human("upgrade-org", "owner");
    let target = human("upgrade-org", "target");
    sqlx::query("INSERT INTO mq_threads (thread_id,org_id,scope_kind,scope_id) VALUES ($1,'upgrade-org','org','upgrade-org')")
        .bind(thread.0).execute(&pool).await.unwrap();
    for (id, role, caps) in [
        ("owner", "owner", vec!["read", "publish", "invite", "close"]),
        ("target", "member", vec!["read", "publish"]),
    ] {
        sqlx::query("INSERT INTO mq_participants (thread_id,principal_kind,principal_id,org_id,role,caps) VALUES ($1,'human',$2,'upgrade-org',$3,$4)")
            .bind(thread.0).bind(id).bind(role).bind(caps).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO mq_messages (message_id,thread_id,org_id,seq,kind,body,sender_kind,sender_id,sender_org_id) VALUES ($1,$2,'upgrade-org',1,'notice','before upgrade','human','owner','upgrade-org')")
        .bind(message).bind(thread.0).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mq_delivery_jobs (job_id,message_id,thread_id,recipient_kind,recipient_id,recipient_org_id) VALUES ($1,$2,$3,'human','target','upgrade-org')")
        .bind(job).bind(message).bind(thread.0).execute(&pool).await.unwrap();

    let mq = Fabric::from_store(Arc::new(PostgresStore::connect(&url).await.unwrap()));
    mq.validate_grant_generation(&owner, thread, 0).await.unwrap();
    mq.validate_grant_generation(&target, thread, 0).await.unwrap();
    let messages = mq.read_messages(&target, thread, 0, 10).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message_id.0, message);
    assert_eq!(messages[0].body, "before upgrade");
    let jobs = mq.claim_delivery_jobs(10).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].job_id.0, job);
    assert!(sqlx::query("UPDATE mq_participants SET grant_generation=-1 WHERE thread_id=$1")
        .bind(thread.0).execute(&pool).await.is_err());
    mq.set_participant_role(&owner, thread, &target, Role::Revoked).await.unwrap();
    mq.set_participant_role(&owner, thread, &target, Role::Member).await.unwrap();
    mq.validate_grant_generation(&target, thread, 1).await.unwrap();
    assert!(mq.validate_grant_generation(&target, thread, 0).await.is_err());
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires DATABASE_URL Postgres"]
async fn postgres_effort_judgment_and_jobs() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let store = PostgresStore::connect(&url).await.expect("connect+migrate");
    let mq = Fabric::from_store(Arc::new(store));

    let org = format!("org-{}", uuid::Uuid::new_v4());
    let a = async_intern(&org, "intern-a");
    let h = human(&org, "user-1");

    let thread = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: org.clone(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "effort-1".into(),
                },
                title: Some("pg judgment".into()),
                participants: vec![
                    Participant::new(a.clone(), Role::Owner),
                    Participant::new(h.clone(), Role::Member),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

    mq.publish(
        &a,
        thread.thread_id,
        PublishMessage {
            kind: MessageKind::Ask,
            body: "metric?".into(),
            idempotency_key: Some(format!("ask-{}", thread.thread_id.0)),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let jobs = mq.claim_delivery_jobs(10).await.unwrap();
    assert!(jobs.iter().any(|j| j.recipient == h));
    for j in jobs {
        mq.settle_delivery_job(j.job_id, j.attempts, DeliveryStatus::Delivered)
            .await
            .unwrap();
    }

    let page = mq.read_messages(&h, thread.thread_id, 0, 10).await.unwrap();
    assert_eq!(page.len(), 1);
}

/// Deliberately opt-in: executes the forward migration on a disposable database.
#[tokio::test]
#[ignore = "requires isolated disposable DATABASE_URL Postgres"]
async fn postgres_atomic_acceptance_replay_and_worker_lease() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let store = Arc::new(PostgresStore::connect(&url).await.unwrap());
    let mq = Fabric::from_store(store.clone());
    let org = format!("org-{}", uuid::Uuid::new_v4());
    let owner = human(&org, "owner");
    let recipient = async_intern(&org, "recipient");
    let thread = mq
        .create_thread(
            &owner,
            CreateThread {
                org_id: org.clone(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "atomic-fixture".into(),
                },
                title: None,
                idempotency_key: None,
                participants: vec![
                    Participant::new(owner.clone(), Role::Owner),
                    Participant::new(recipient.clone(), Role::Agent),
                ],
            },
        )
        .await
        .unwrap();
    let request = PublishMessage {
        body: "original".into(),
        idempotency_key: Some("same-key".into()),
        ..Default::default()
    };
    let wrong_org = human("different-org", "recipient");
    assert!(store
        .append_with_delivery(thread.thread_id, &owner, request.clone(), &[wrong_org])
        .await
        .is_err());
    assert!(store
        .read_messages(thread.thread_id, 0, 100)
        .await
        .unwrap()
        .is_empty());
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let mq = mq.clone();
        let owner = owner.clone();
        let request = request.clone();
        tasks.spawn(async move { mq.publish(&owner, thread.thread_id, request).await.unwrap() });
    }
    let mut ids = std::collections::HashSet::new();
    while let Some(result) = tasks.join_next().await {
        ids.insert(result.unwrap().message_id);
    }
    assert_eq!(ids.len(), 1);
    let mut changed = request.clone();
    changed.body = "changed".into();
    assert!(matches!(
        mq.publish(&owner, thread.thread_id, changed).await,
        Err(Error::Conflict(_))
    ));
    let mut tasks = tokio::task::JoinSet::new();
    for n in 0..8 {
        let mq = mq.clone();
        let owner = owner.clone();
        let mut request = request.clone();
        request.idempotency_key = Some(format!("unique-{n}"));
        tasks.spawn(async move { mq.publish(&owner, thread.thread_id, request).await.unwrap() });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    let page = store.read_messages(thread.thread_id, 0, 100).await.unwrap();
    assert_eq!(
        page.iter().map(|m| m.seq).collect::<Vec<_>>(),
        (1..=9).collect::<Vec<_>>()
    );
    // The database must be disposable and otherwise idle: claims are worker-global.
    let jobs = store.claim_delivery_jobs(100).await.unwrap();
    let own_jobs: Vec<_> = jobs.iter().filter(|j| j.recipient == recipient).collect();
    assert_eq!(own_jobs.len(), 9);
    assert!(store.claim_delivery_jobs(100).await.unwrap().is_empty());
    let job = own_jobs[0];
    assert!(matches!(
        store
            .settle_delivery_job(job.job_id, job.attempts + 1, DeliveryStatus::Dispatched)
            .await,
        Err(Error::Conflict(_))
    ));
    store
        .settle_delivery_job(job.job_id, job.attempts, DeliveryStatus::Dispatched)
        .await
        .unwrap();
    assert!(matches!(
        store
            .settle_delivery_job(job.job_id, job.attempts, DeliveryStatus::Dispatched)
            .await,
        Err(Error::Conflict(_))
    ));
}
