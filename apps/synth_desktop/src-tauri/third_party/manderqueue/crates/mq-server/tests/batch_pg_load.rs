//! Integration: measure PG write load sync vs Redis/memory batch flush.
//!
//! ```bash
//! docker compose up -d postgres redis
//! DATABASE_URL=postgres://mq:mq@127.0.0.1:5433/manderqueue \
//! REDIS_URL=redis://127.0.0.1:6380 \
//!   cargo test -p mq-server --test batch_pg_load -- --ignored --nocapture
//! ```

use std::sync::Arc;

use mq_core::*;
use mq_server::postgres::{PgWriteMetrics, PostgresStore};
use mq_server::write_buffer::RedisWriteBuffer;

fn human(org: &str, id: &str) -> Principal {
    Principal {
        kind: PrincipalKind::Human,
        id: id.into(),
        org_id: org.into(),
    }
}

async fn seed(mq: &Fabric, org: &str) -> (Principal, Principal, ThreadId) {
    let a = human(org, "pub");
    let b = human(org, "sub");
    let t = mq
        .create_thread(
            &a,
            CreateThread {
                org_id: org.into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Org,
                    id: "load".into(),
                },
                title: Some("pg load".into()),
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

const N: u64 = 40;

#[tokio::test]
#[ignore = "requires DATABASE_URL Postgres"]
async fn sync_publish_is_linear_in_pg_write_txns() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let metrics = Arc::new(PgWriteMetrics::default());
    let store = PostgresStore::connect(&url)
        .await
        .expect("connect")
        .with_metrics(metrics.clone());
    let mq = Fabric::from_store(Arc::new(store));

    let org = format!("sync-{}", uuid::Uuid::new_v4());
    let (a, _, tid) = seed(&mq, &org).await;
    metrics.reset();

    for i in 0..N {
        mq.publish(
            &a,
            tid,
            PublishMessage {
                kind: MessageKind::Notice,
                body: format!("s{i}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let snap = metrics.snapshot();
    eprintln!("sync path: {snap:?} for N={N}");
    // Each publish atomically commits message and delivery intent.
    assert_eq!(snap.write_txns, N);
    assert_eq!(snap.message_rows, N);
    assert_eq!(snap.job_rows, N);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL Postgres"]
async fn product_publish_bypasses_volatile_memory_batch() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let metrics = Arc::new(PgWriteMetrics::default());
    let durable = Arc::new(
        PostgresStore::connect(&url)
            .await
            .expect("connect")
            .with_metrics(metrics.clone()),
    );
    let batching = Arc::new(BatchingStore::new(durable.clone()));
    let mq = Fabric::from_store(batching.clone());

    let org = format!("batch-{}", uuid::Uuid::new_v4());
    let (a, b, tid) = seed(&mq, &org).await;
    metrics.reset();

    for i in 0..N {
        mq.publish(
            &a,
            tid,
            PublishMessage {
                kind: MessageKind::Notice,
                body: format!("b{i}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    assert_eq!(metrics.snapshot().write_txns, N, "accepted before reply");
    assert_eq!(batching.pending_len().await, 0);

    // Read durable acknowledged messages without needing a flush.
    let page = mq.read_messages(&b, tid, 0, 200).await.unwrap();
    assert_eq!(page.len(), N as usize);

    let flushed = batching.flush_all().await.unwrap();
    assert_eq!(flushed, 0);

    let snap = metrics.snapshot();
    eprintln!("batch path: {snap:?} for N={N}");
    assert_eq!(
        snap.write_txns, N,
        "product acknowledgments each require an atomic durable transaction"
    );
    assert_eq!(snap.message_rows, N);
    assert_eq!(snap.job_rows, N);

    let jobs_for_thread: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mq_delivery_jobs WHERE thread_id = $1")
            .bind(tid.0)
            .fetch_one(durable.pool())
            .await
            .unwrap();
    assert_eq!(jobs_for_thread as u64, N);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL Postgres and REDIS_URL"]
async fn redis_buffer_batch_flush_collapses_pg_write_txns() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL");
    let metrics = Arc::new(PgWriteMetrics::default());
    let durable = Arc::new(
        PostgresStore::connect(&url)
            .await
            .expect("connect")
            .with_metrics(metrics.clone()),
    );
    let mq = Fabric::from_store(durable.clone());
    let buffer = RedisWriteBuffer::connect(&redis_url).expect("redis");
    let _ = buffer.clear().await;

    let org = format!("redis-{}", uuid::Uuid::new_v4());
    let (a, b, tid) = seed(&mq, &org).await;

    // Build buffered publishes the way a hot path would stage them.
    let mut staged = Vec::new();
    for i in 0..N {
        let msg = Message {
            message_id: MessageId::new(),
            thread_id: tid,
            seq: i + 1,
            kind: MessageKind::Notice,
            body: format!("r{i}"),
            payload: serde_json::json!({}),
            sender: a.clone(),
            idempotency_key: None,
            correlation_id: None,
            parent_message_id: None,
            causation_id: None,
            created_at: chrono::Utc::now(),
        };
        let item = BufferedPublish {
            org_id: org.clone(),
            message: msg,
            recipients: vec![b.clone()],
        };
        buffer.push(&item).await.expect("push");
        staged.push(item);
    }

    assert_eq!(buffer.len().await.unwrap(), N as usize);
    metrics.reset();

    let batch = buffer.drain(N as usize).await.expect("drain");
    assert_eq!(batch.len(), N as usize);
    durable.flush_write_batch(&batch).await.unwrap();

    let snap = metrics.snapshot();
    eprintln!("redis→pg batch: {snap:?} for N={N}");
    assert_eq!(snap.write_txns, 1);
    assert_eq!(snap.message_rows, N);
    assert_eq!(snap.job_rows, N);
    assert_eq!(buffer.len().await.unwrap(), 0);

    let page = mq.read_messages(&b, tid, 0, 200).await.unwrap();
    assert_eq!(page.len(), N as usize);
    assert_eq!(staged.len(), page.len());
}
