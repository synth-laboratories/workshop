//! Unique fabric invariants — what makes MQ different from a generic chat bus.

use mq_core::*;

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

fn actor(org: &str, id: &str) -> Principal {
    Principal {
        kind: PrincipalKind::Actor,
        id: id.into(),
        org_id: org.into(),
    }
}

#[tokio::test]
async fn effort_judgment_thread_does_not_require_a_run() {
    let mq = Fabric::memory();
    let org = "org-1";
    let a = async_intern(org, "intern-a");
    let h = human(org, "user-1");

    let thread = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "effort-craftax".into(),
                },
                title: Some("metric judgment".into()),
                participants: vec![
                    Participant::new(a.clone(), Role::Owner),
                    Participant::new(h.clone(), Role::Member),
                ],
                idempotency_key: None,
            },
        )
        .await
        .expect("create effort thread");

    assert_eq!(thread.scope.kind, ScopeKind::Effort);

    let ask = mq
        .publish(
            &a,
            thread.thread_id,
            PublishMessage {
                kind: MessageKind::Ask,
                body: "dense or sparse metric?".into(),
                idempotency_key: Some("ask-1".into()),
                ..Default::default()
            },
        )
        .await
        .expect("ask");

    let answer = mq
        .publish(
            &h,
            thread.thread_id,
            PublishMessage {
                kind: MessageKind::Answer,
                body: "dense + success".into(),
                correlation_id: Some(ask.message_id.0.to_string()),
                idempotency_key: Some("ans-1".into()),
                ..Default::default()
            },
        )
        .await
        .expect("answer");

    let page = mq
        .read_messages(&a, thread.thread_id, 0, 10)
        .await
        .expect("read");
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].kind, MessageKind::Ask);
    assert_eq!(page[1].message_id, answer.message_id);

    let jobs = mq.claim_delivery_jobs(10).await.unwrap();
    assert!(
        jobs.iter().any(|j| j.recipient == h),
        "ask should enqueue delivery to human"
    );
}

#[tokio::test]
async fn non_member_cannot_read_or_publish_fail_closed() {
    let mq = Fabric::memory();
    let org = "org-1";
    let a = async_intern(org, "intern-a");
    let stranger = human(org, "eavesdropper");

    let thread = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "e1".into(),
                },
                title: None,
                participants: vec![Participant::new(a.clone(), Role::Owner)],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

    let err = mq
        .read_messages(&stranger, thread.thread_id, 0, 10)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)));

    let err = mq
        .publish(
            &stranger,
            thread.thread_id,
            PublishMessage {
                kind: MessageKind::Notice,
                body: "nope".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)));
}

#[tokio::test]
async fn cross_org_principal_forbidden() {
    let mq = Fabric::memory();
    let a = async_intern("org-1", "intern-a");
    let other = human("org-2", "user-x");

    let err = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: "org-1".into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "e1".into(),
                },
                title: None,
                participants: vec![
                    Participant::new(a.clone(), Role::Owner),
                    Participant::new(other.clone(), Role::Member),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Forbidden("org_workspace_mismatch")));
}

#[tokio::test]
async fn org_workspaces_are_hard_isolated() {
    let mq = Fabric::memory();
    let a1 = human("org-a", "u1");
    let a2 = human("org-a", "u2");
    let b1 = human("org-b", "u1");

    let thread_a = mq
        .create_thread(
            &a1,
            CreateThread {
                org_id: "org-a".into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "e-a".into(),
                },
                title: Some("a only".into()),
                participants: vec![
                    Participant::new(a1.clone(), Role::Owner),
                    Participant::new(a2.clone(), Role::Member),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

    mq.publish(
        &a1,
        thread_a.thread_id,
        PublishMessage {
            kind: MessageKind::Notice,
            body: "secret-to-org-a".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Org B cannot see the thread (not found, not 403 leak with content).
    let err = mq.get_thread(&b1, thread_a.thread_id).await.unwrap_err();
    assert!(matches!(err, Error::NotFound("thread")));

    let err = mq
        .read_messages(&b1, thread_a.thread_id, 0, 10)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound("thread")));

    let err = mq
        .publish(
            &b1,
            thread_a.thread_id,
            PublishMessage {
                kind: MessageKind::Notice,
                body: "intrusion".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound("thread")));

    assert!(mq.list_threads(&b1, None).await.unwrap().is_empty());
    let visible = mq.list_threads(&a1, None).await.unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].thread_id, thread_a.thread_id);
}

#[tokio::test]
async fn human_async_and_actor_share_same_thread() {
    let mq = Fabric::memory();
    let org = "org-1";
    let h = human(org, "user-1");
    let a = async_intern(org, "intern-a");
    let act = actor(org, "actor-1");

    let thread = mq
        .create_thread(
            &h,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Project,
                    id: "swarm-1".into(),
                },
                title: Some("swarm auto-thread".into()),
                participants: vec![
                    Participant::new(h.clone(), Role::Owner),
                    Participant::new(a.clone(), Role::Agent),
                    Participant::new(act.clone(), Role::Agent),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

    mq.publish(
        &h,
        thread.thread_id,
        PublishMessage {
            kind: MessageKind::Steer,
            body: "explore more".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let for_actor = mq
        .read_messages(&act, thread.thread_id, 0, 10)
        .await
        .unwrap();
    let for_async = mq.read_messages(&a, thread.thread_id, 0, 10).await.unwrap();
    assert_eq!(for_actor.len(), 1);
    assert_eq!(for_async[0].kind, MessageKind::Steer);

    let jobs = mq.claim_delivery_jobs(10).await.unwrap();
    assert_eq!(jobs.len(), 2, "steer fans out to async + actor");
    for job in jobs {
        mq.settle_delivery_job(job.job_id, job.attempts, DeliveryStatus::Delivered)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn idempotent_publish_returns_same_message() {
    let mq = Fabric::memory();
    let org = "org-1";
    let a = async_intern(org, "intern-a");

    let thread = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "e1".into(),
                },
                title: None,
                participants: vec![Participant::new(a.clone(), Role::Owner)],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

    let req = PublishMessage {
        kind: MessageKind::Ask,
        body: "once".into(),
        idempotency_key: Some("idem-42".into()),
        ..Default::default()
    };
    let m1 = mq.publish(&a, thread.thread_id, req.clone()).await.unwrap();
    let m2 = mq.publish(&a, thread.thread_id, req).await.unwrap();
    assert_eq!(m1.message_id, m2.message_id);
    assert_eq!(m1.seq, m2.seq);

    let page = mq.read_messages(&a, thread.thread_id, 0, 10).await.unwrap();
    assert_eq!(page.len(), 1);
}

#[tokio::test]
async fn list_threads_hides_threads_caller_cannot_read() {
    let mq = Fabric::memory();
    let org = "org-1";
    let a = async_intern(org, "intern-a");
    let h = human(org, "user-1");

    mq.create_thread(
        &a,
        CreateThread {
            org_id: org.into(),
            scope: ScopeBinding {
                kind: ScopeKind::Effort,
                id: "private".into(),
            },
            title: None,
            participants: vec![Participant::new(a.clone(), Role::Owner)],
            idempotency_key: None,
        },
    )
    .await
    .unwrap();

    let visible = mq.list_threads(&h, None).await.unwrap();
    assert!(visible.is_empty());
}

#[tokio::test]
async fn invite_required_to_add_participant() {
    let mq = Fabric::memory();
    let org = "org-1";
    let a = async_intern(org, "intern-a");
    let h = human(org, "user-1");
    let h2 = human(org, "user-2");

    let thread = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "e1".into(),
                },
                title: None,
                participants: vec![
                    Participant::new(a.clone(), Role::Owner),
                    Participant::new(h.clone(), Role::Member),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

    // Member lacks invite.
    let err = mq
        .add_participant(&h, thread.thread_id, Participant::new(h2, Role::Agent))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Forbidden(_)));
}

#[tokio::test]
async fn ensure_thread_is_idempotent_by_key() {
    let mq = Fabric::memory();
    let org = "org-1";
    let a = async_intern(org, "intern-a");
    let h = human(org, "user-1");

    let req = CreateThread {
        org_id: org.into(),
        scope: ScopeBinding {
            kind: ScopeKind::Effort,
            id: "effort-bind".into(),
        },
        title: Some("binding".into()),
        participants: vec![
            Participant::new(a.clone(), Role::Owner),
            Participant::new(h.clone(), Role::Member),
        ],
        idempotency_key: Some("smr:run:run-42".into()),
    };

    let t1 = mq.ensure_thread(&a, req.clone()).await.unwrap();
    let t2 = mq.ensure_thread(&a, req.clone()).await.unwrap();
    assert_eq!(t1.thread_id, t2.thread_id);
    assert_eq!(t1.idempotency_key.as_deref(), Some("smr:run:run-42"));

    let err = mq
        .ensure_thread(
            &a,
            CreateThread {
                idempotency_key: None,
                ..req
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::Invalid("idempotency_key_required_for_ensure")
    ));
}

#[tokio::test]
async fn directed_recipients_only_enqueue_named_members() {
    let mq = Fabric::memory();
    let org = "org-1";
    let h = human(org, "user-1");
    let a = async_intern(org, "intern-a");
    let act = actor(org, "actor-1");
    let other = human(org, "user-2");

    let thread = mq
        .create_thread(
            &h,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Project,
                    id: "swarm".into(),
                },
                title: None,
                participants: vec![
                    Participant::new(h.clone(), Role::Owner),
                    Participant::new(a.clone(), Role::Agent),
                    Participant::new(act.clone(), Role::Agent),
                    Participant::new(other.clone(), Role::Member),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

    mq.publish(
        &h,
        thread.thread_id,
        PublishMessage {
            kind: MessageKind::Steer,
            body: "only actor".into(),
            recipients: vec![act.clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let jobs = mq.claim_delivery_jobs(10).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].recipient, act);

    let err = mq
        .publish(
            &h,
            thread.thread_id,
            PublishMessage {
                kind: MessageKind::Steer,
                body: "ghost".into(),
                recipients: vec![human(org, "not-a-member")],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Invalid("recipient_not_a_member")));
}

#[tokio::test]
async fn parent_and_causation_correlate_replies() {
    let mq = Fabric::memory();
    let org = "org-1";
    let a = async_intern(org, "intern-a");
    let h = human(org, "user-1");

    let thread = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "e-corr".into(),
                },
                title: None,
                participants: vec![
                    Participant::new(a.clone(), Role::Owner),
                    Participant::new(h.clone(), Role::Member),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

    let ask = mq
        .publish(
            &a,
            thread.thread_id,
            PublishMessage {
                kind: MessageKind::Ask,
                body: "q?".into(),
                causation_id: Some("interaction-9".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(ask.causation_id.as_deref(), Some("interaction-9"));

    let answer = mq
        .publish(
            &h,
            thread.thread_id,
            PublishMessage {
                kind: MessageKind::Answer,
                body: "a.".into(),
                parent_message_id: Some(ask.message_id),
                causation_id: Some("interaction-9".into()),
                correlation_id: Some(ask.message_id.0.to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(answer.parent_message_id, Some(ask.message_id));
    assert_eq!(answer.causation_id.as_deref(), Some("interaction-9"));

    let page = mq.read_messages(&a, thread.thread_id, 0, 10).await.unwrap();
    assert_eq!(page[1].parent_message_id, Some(ask.message_id));
}

#[tokio::test]
async fn moderator_cannot_demote_owner_or_grant_peer_authority() {
    let mq = Fabric::memory();
    let owner = human("org", "owner");
    let moderator = human("org", "moderator");
    let member = human("org", "member");
    let thread = mq
        .create_thread(
            &owner,
            CreateThread {
                org_id: "org".into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "e1".into(),
                },
                title: None,
                participants: vec![
                    Participant::new(owner.clone(), Role::Owner),
                    Participant::new(moderator.clone(), Role::Moderator),
                    Participant::new(member.clone(), Role::Member),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();
    for (target, role) in [
        (&owner, Role::Observer),
        (&moderator, Role::Observer),
        (&member, Role::Moderator),
    ] {
        assert!(matches!(
            mq.set_participant_role(&moderator, thread.thread_id, target, role)
                .await,
            Err(Error::Forbidden(_))
        ));
    }
    assert!(matches!(
        mq.add_participant(
            &moderator,
            thread.thread_id,
            Participant::new(human("org", "new-moderator"), Role::Moderator)
        )
        .await,
        Err(Error::Forbidden(_))
    ));
    mq.set_participant_role(&moderator, thread.thread_id, &member, Role::Observer)
        .await
        .unwrap();
    mq.set_participant_role(&owner, thread.thread_id, &member, Role::Moderator)
        .await
        .unwrap();
}
