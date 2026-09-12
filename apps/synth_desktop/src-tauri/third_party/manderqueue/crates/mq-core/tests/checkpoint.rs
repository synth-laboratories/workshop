use std::sync::Arc;
use mq_core::*;

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
