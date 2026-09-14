//! Product publish must durably accept message and delivery intent before returning.

use std::sync::Arc;

use mq_core::*;

fn human(org: &str, id: &str) -> Principal {
    Principal {
        kind: PrincipalKind::Human,
        id: id.into(),
        org_id: org.into(),
    }
}

async fn seed_thread(mq: &Fabric, org: &str) -> (Principal, Principal, ThreadId) {
    let a = human(org, "a");
    let b = human(org, "b");
    let t = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Org,
                    id: "o".into(),
                },
                title: Some("batch".into()),
                participants: vec![
                    Participant::new(a.clone(), Role::Owner),
                    Participant::new(b.clone(), Role::Agent),
                ],
                idempotency_key: None,
            },
        )
        .await
        .unwrap();
    (a, b, t.thread_id)
}

#[tokio::test]
async fn product_publish_bypasses_volatile_batch_buffer() {
    let counters = Arc::new(StoreCounters::default());
    let durable = Arc::new(MeteredStore::with_counters(
        Arc::new(MemoryStore::default()),
        counters.clone(),
    ));
    let batching = Arc::new(BatchingStore::new(durable));
    let mq = Fabric::from_store(batching.clone());

    let (a, _b, tid) = seed_thread(&mq, "org-batch").await;
    const N: u64 = 50;

    for i in 0..N {
        mq.publish(
            &a,
            tid,
            PublishMessage {
                kind: MessageKind::Notice,
                body: format!("m{i}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let before = counters.snapshot();
    assert_eq!(
        before.append_calls, N,
        "all acknowledgments require atomic durable acceptance"
    );
    assert_eq!(before.enqueue_calls, 0, "no separate enqueue crash window");
    assert_eq!(before.flush_batch_calls, 0);
    assert_eq!(batching.pending_len().await, 0);
    assert_eq!(batching.flush_all().await.unwrap(), 0);
    assert_eq!(mq.claim_delivery_jobs(100).await.unwrap().len(), N as usize);

    let page = mq.read_messages(&a, tid, 0, 200).await.unwrap();
    assert_eq!(page.len(), N as usize);
}

#[tokio::test]
async fn sync_path_uses_one_atomic_acceptance_call() {
    let counters = Arc::new(StoreCounters::default());
    let durable = Arc::new(MeteredStore::with_counters(
        Arc::new(MemoryStore::default()),
        counters.clone(),
    ));
    let mq = Fabric::from_store(durable);

    let (a, _b, tid) = seed_thread(&mq, "org-sync").await;
    const N: u64 = 50;

    for i in 0..N {
        mq.publish(
            &a,
            tid,
            PublishMessage {
                kind: MessageKind::Notice,
                body: format!("m{i}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let snap = counters.snapshot();
    assert_eq!(snap.append_calls, N);
    assert_eq!(
        snap.enqueue_calls, 0,
        "delivery intent is part of atomic append"
    );
    assert_eq!(snap.flush_batch_calls, 0);
    assert_eq!(mq.claim_delivery_jobs(100).await.unwrap().len(), N as usize);
}

#[tokio::test]
async fn product_acceptance_is_visible_without_flush() {
    let batching = Arc::new(BatchingStore::new(Arc::new(MemoryStore::default())));
    let mq = Fabric::from_store(batching.clone());
    let (a, b, tid) = seed_thread(&mq, "org-ryw").await;

    mq.publish(
        &a,
        tid,
        PublishMessage {
            kind: MessageKind::Ask,
            body: "pending?".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(batching.pending_len().await, 0);
    let page = mq.read_messages(&b, tid, 0, 10).await.unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].body, "pending?");
}
