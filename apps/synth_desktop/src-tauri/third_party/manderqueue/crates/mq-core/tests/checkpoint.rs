use std::sync::Arc;
use mq_core::*;

#[tokio::test]
async fn revocation_generation_survives_restoration_and_checkpoint() {
    let store = MemoryStore::default();
    let fabric = Fabric::from_store(Arc::new(store.clone()));
    let owner = Principal { kind: PrincipalKind::Human, org_id: "org".into(), id: "owner".into() };
    let recipient = Principal { kind: PrincipalKind::Actor, org_id: "org".into(), id: "recipient".into() };
    let mut supplied = Participant::new(recipient.clone(), Role::Agent);
    supplied.grant_generation = 999;
    let thread = fabric.create_thread(&owner, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() },
        title: None, idempotency_key: None,
        participants: vec![Participant::new(owner.clone(), Role::Owner), supplied],
    }).await.unwrap().thread_id;
    let generation = |members: Vec<Participant>| members.into_iter().find(|p|p.principal==recipient).unwrap().grant_generation;
    assert_eq!(generation(store.list_participants(thread).await.unwrap()),0);
    fabric.validate_grant_generation(&recipient,thread,0).await.unwrap();
    fabric.set_participant_role(&owner,thread,&recipient,Role::Revoked).await.unwrap();
    fabric.set_participant_role(&owner,thread,&recipient,Role::Revoked).await.unwrap();
    assert_eq!(generation(store.list_participants(thread).await.unwrap()),1);
    fabric.set_participant_role(&owner,thread,&recipient,Role::Agent).await.unwrap();
    assert_eq!(generation(store.list_participants(thread).await.unwrap()),1);
    let restored = MemoryStore::from_checkpoint(serde_json::from_slice(&serde_json::to_vec(&store.checkpoint()).unwrap()).unwrap()).unwrap();
    let stale = PublishMessage { body: "checked before revoke".into(), expected_grant_generation: Some(0), ..Default::default() };
    assert!(store.append_with_delivery(thread,&recipient,stale.clone(),&[]).await.is_err());
    assert!(store.read_messages(thread,0,10).await.unwrap().is_empty());
    let current = PublishMessage { expected_grant_generation: Some(1), ..stale };
    store.append_with_delivery(thread,&recipient,current,&[]).await.unwrap();
    let wire: PublishMessage = serde_json::from_value(serde_json::json!({"kind":"notice","body":"wire","expected_grant_generation":99})).unwrap();
    assert_eq!(wire.expected_grant_generation,None);
    assert_eq!(generation(restored.list_participants(thread).await.unwrap()),1);
    let recovered = Fabric::from_store(Arc::new(restored.clone()));
    recovered.set_participant_role(&owner,thread,&recipient,Role::Revoked).await.unwrap();
    assert_eq!(generation(restored.list_participants(thread).await.unwrap()),2);
}

#[tokio::test]
async fn cold_restore_preserves_ids_dedup_jobs_and_isolates_branches() {
    let store = MemoryStore::default();
    let fabric = Fabric::from_store(Arc::new(store.clone()));
    let owner = Principal { kind: PrincipalKind::System, org_id: "checkpoint".into(), id: "owner".into() };
    let recipient = Principal { kind: PrincipalKind::Actor, org_id: owner.org_id.clone(), id: "a".into() };
    let thread = fabric.create_thread(&owner, CreateThread {
        org_id: owner.org_id.clone(), scope: ScopeBinding { kind: ScopeKind::Project, id: "p".into() },
        title: None, idempotency_key: Some("thread".into()),
        participants: vec![Participant::new(owner.clone(), Role::Owner), Participant::new(recipient.clone(), Role::Agent)],
    }).await.unwrap();
    let publish = PublishMessage { body: "before".into(), idempotency_key: Some("message".into()), ..Default::default() };
    let message = fabric.publish(&owner, thread.thread_id, publish.clone()).await.unwrap();
    let encoded = serde_json::to_vec(&store.checkpoint()).unwrap();
    let a = MemoryStore::from_checkpoint(serde_json::from_slice(&encoded).unwrap()).unwrap();
    let b = MemoryStore::from_checkpoint(serde_json::from_slice(&encoded).unwrap()).unwrap();
    assert_eq!(serde_json::to_vec(&a.checkpoint()).unwrap(), encoded);
    let af = Fabric::from_store(Arc::new(a.clone()));
    assert_eq!(af.publish(&owner, thread.thread_id, publish).await.unwrap(), message);
    let after = af.publish(&owner, thread.thread_id, PublishMessage { body: "branch a".into(), ..Default::default() }).await.unwrap();
    assert_eq!(after.seq, 2);
    assert_eq!(b.read_messages(thread.thread_id, 0, 10).await.unwrap().len(), 1);
    assert_eq!(store.read_messages(thread.thread_id, 0, 10).await.unwrap().len(), 1);
    assert_eq!(b.checkpoint().jobs.len(), 1);
    assert_eq!(b.checkpoint().jobs[0].recipient, recipient);
    let mut invalid = b.checkpoint(); invalid.threads[0].2[0].seq = 5;
    assert!(MemoryStore::from_checkpoint(invalid).is_err());
}
