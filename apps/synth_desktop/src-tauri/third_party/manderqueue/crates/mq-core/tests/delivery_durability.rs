//! Atomic acceptance/replay and leased-worker invariants, without services or wall-clock sleeps.
use chrono::{Duration, Utc};
use mq_core::*;
use std::sync::Arc;

async fn fixture() -> (MemoryStore, Fabric, Principal, Principal, ThreadId) {
    let store = MemoryStore::default();
    let fabric = Fabric::from_store(Arc::new(store.clone()));
    let owner = Principal {
        kind: PrincipalKind::Human,
        id: "owner".into(),
        org_id: "org".into(),
    };
    let actor = Principal {
        kind: PrincipalKind::Actor,
        id: "actor".into(),
        org_id: "org".into(),
    };
    let thread = fabric
        .create_thread(
            &owner,
            CreateThread {
                org_id: "org".into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Effort,
                    id: "effort".into(),
                },
                title: None,
                idempotency_key: None,
                participants: vec![
                    Participant::new(owner.clone(), Role::Owner),
                    Participant::new(actor.clone(), Role::Agent),
                ],
            },
        )
        .await
        .unwrap();
    (store, fabric, owner, actor, thread.thread_id)
}
fn request() -> PublishMessage {
    PublishMessage {
        body: "known answer".into(),
        idempotency_key: Some("stable-key".into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn failure_cannot_leave_message_without_outbox() {
    let (store, _, owner, mut actor, thread) = fixture().await;
    actor.org_id = "wrong-org".into();
    assert!(store
        .append_with_delivery(thread, &owner, request(), &[actor])
        .await
        .is_err());
    assert!(store
        .read_messages(thread, 0, 100)
        .await
        .unwrap()
        .is_empty());
    assert!(store.checkpoint().jobs.is_empty());
}

#[tokio::test]
async fn changed_semantics_conflict_and_other_publishers_can_reuse_key() {
    let (store, fabric, owner, actor, thread) = fixture().await;
    let first = fabric.publish(&owner, thread, request()).await.unwrap();
    let mut changed = request();
    changed.body = "changed".into();
    assert!(matches!(
        fabric.publish(&owner, thread, changed).await,
        Err(Error::Conflict(_))
    ));
    let mut changed = request();
    changed.recipients = vec![actor.clone()];
    assert!(matches!(
        fabric.publish(&owner, thread, changed).await,
        Err(Error::Conflict(_))
    ));
    let other = fabric.publish(&actor, thread, request()).await.unwrap();
    assert_ne!(first.message_id, other.message_id);
    assert_eq!(store.checkpoint().jobs.len(), 2);
}

#[tokio::test]
async fn concurrent_acceptance_has_contiguous_sequences_and_atomic_intent() {
    let (store, fabric, owner, _, thread) = fixture().await;
    let mut tasks = tokio::task::JoinSet::new();
    for n in 0..32 {
        let fabric = fabric.clone();
        let owner = owner.clone();
        tasks.spawn(async move {
            let mut req = request();
            req.idempotency_key = Some(format!("k{n}"));
            fabric.publish(&owner, thread, req).await.unwrap()
        });
    }
    while tasks.join_next().await.is_some() {}
    let messages = store.read_messages(thread, 0, 100).await.unwrap();
    assert_eq!(
        messages.iter().map(|m| m.seq).collect::<Vec<_>>(),
        (1..=32).collect::<Vec<_>>()
    );
    assert_eq!(store.checkpoint().jobs.len(), 32);
    let encoded = serde_json::to_vec(&store.checkpoint()).unwrap();
    let restored = MemoryStore::from_checkpoint(serde_json::from_slice(&encoded).unwrap()).unwrap();
    assert_eq!(serde_json::to_vec(&restored.checkpoint()).unwrap(), encoded);
}

#[tokio::test]
async fn replay_repairs_frozen_intent_without_resetting_completed_jobs() {
    let (store, fabric, owner, actor, thread) = fixture().await;
    let message = fabric.publish(&owner, thread, request()).await.unwrap();
    let claimed = store.claim_delivery_jobs(1).await.unwrap().remove(0);
    store
        .settle_delivery_job(claimed.job_id, claimed.attempts, DeliveryStatus::Dispatched)
        .await
        .unwrap();
    fabric.publish(&owner, thread, request()).await.unwrap();
    assert_eq!(
        store.checkpoint().jobs[0].status,
        DeliveryStatus::Dispatched
    );
    let mut snapshot = store.checkpoint();
    snapshot.jobs.clear();
    let restored = MemoryStore::from_checkpoint(snapshot).unwrap();
    let restored_fabric = Fabric::from_store(Arc::new(restored.clone()));
    let newcomer = Principal {
        id: "newcomer".into(),
        ..actor.clone()
    };
    restored_fabric
        .add_participant(&owner, thread, Participant::new(newcomer, Role::Agent))
        .await
        .unwrap();
    assert_eq!(
        restored_fabric
            .publish(&owner, thread, request())
            .await
            .unwrap()
            .message_id,
        message.message_id
    );
    let jobs = restored.checkpoint().jobs;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].recipient, actor);
}

#[tokio::test]
async fn leases_exclude_other_workers_and_fence_expired_generation() {
    let (store, fabric, owner, _, thread) = fixture().await;
    fabric.publish(&owner, thread, request()).await.unwrap();
    let first = store.claim_delivery_jobs(1).await.unwrap().remove(0);
    assert!(store.claim_delivery_jobs(1).await.unwrap().is_empty());
    let mut snapshot = store.checkpoint();
    snapshot.jobs[0].lease_until = Some(Utc::now() - Duration::seconds(1));
    let recovered = MemoryStore::from_checkpoint(snapshot).unwrap();
    let second = recovered.claim_delivery_jobs(1).await.unwrap().remove(0);
    assert_eq!(second.attempts, first.attempts + 1);
    assert!(matches!(
        recovered
            .settle_delivery_job(first.job_id, first.attempts, DeliveryStatus::Dispatched)
            .await,
        Err(Error::Conflict(_))
    ));
    recovered
        .settle_delivery_job(second.job_id, second.attempts, DeliveryStatus::Pending)
        .await
        .unwrap();
    assert!(recovered.claim_delivery_jobs(1).await.unwrap().is_empty());
    assert!(recovered.checkpoint().jobs[0].next_attempt_at.unwrap() > Utc::now());
}

#[tokio::test]
async fn repeated_invite_is_noop_and_never_changes_existing_role() {
    let (store, fabric, owner, actor, thread) = fixture().await;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let fabric = fabric.clone();
        let owner = owner.clone();
        let actor = actor.clone();
        tasks.spawn(async move {
            fabric
                .add_participant(&owner, thread, Participant::new(actor, Role::Agent))
                .await
                .unwrap()
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    assert_eq!(store.list_participants(thread).await.unwrap().len(), 2);
    assert!(matches!(
        fabric
            .add_participant(&owner, thread, Participant::new(actor, Role::Member))
            .await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn demoted_inviter_loses_authority_and_owner_is_preserved() {
    let (store, fabric, owner, actor, thread) = fixture().await;
    fabric
        .set_participant_role(&owner, thread, &actor, Role::Moderator)
        .await
        .unwrap();
    let newcomer = Principal {
        id: "new".into(),
        ..actor.clone()
    };
    fabric
        .add_participant(
            &actor,
            thread,
            Participant::new(newcomer.clone(), Role::Agent),
        )
        .await
        .unwrap();
    fabric
        .set_participant_role(&owner, thread, &actor, Role::Member)
        .await
        .unwrap();
    assert!(matches!(
        fabric
            .set_participant_role(&actor, thread, &newcomer, Role::Member)
            .await,
        Err(Error::Forbidden(_))
    ));
    assert!(matches!(
        fabric
            .set_participant_role(&owner, thread, &owner, Role::Member)
            .await,
        Err(Error::Forbidden(_))
    ));
    let members = store.list_participants(thread).await.unwrap();
    assert_eq!(members.iter().filter(|p| p.role == Role::Owner).count(), 1);
}
