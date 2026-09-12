//! Redis PUBLISH wake — best-effort; never required for correctness.

use async_trait::async_trait;
use mq_core::{ThreadId, Wake};

#[derive(Clone)]
pub struct RedisWake {
    client: redis::Client,
}

impl RedisWake {
    pub fn connect(redis_url: &str) -> Result<Self, redis::RedisError> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
        })
    }

    /// Forward Redis pubsub into a local broadcast hub (for SSE across processes).
    pub async fn pipe_into_local(
        &self,
        local: mq_core::LocalWake,
    ) -> Result<(), redis::RedisError> {
        let client = self.client.clone();
        tokio::spawn(async move {
            let Ok(mut pubsub) = client.get_async_pubsub().await else {
                return;
            };
            if pubsub.psubscribe("mq:wake:*").await.is_err() {
                return;
            }
            use futures_util::StreamExt;
            let mut stream = pubsub.on_message();
            while let Some(msg) = stream.next().await {
                let channel: String = msg.get_channel_name().to_string();
                if channel == "mq:wake:worker" {
                    local.notify_worker().await;
                } else if let Some(id) = channel.strip_prefix("mq:wake:thread:") {
                    if let Ok(uuid) = uuid::Uuid::parse_str(id) {
                        local.notify_thread(ThreadId(uuid)).await;
                    }
                }
            }
        });
        Ok(())
    }
}

#[async_trait]
impl Wake for RedisWake {
    async fn notify_thread(&self, thread_id: ThreadId) {
        if let Ok(mut conn) = self.client.get_multiplexed_async_connection().await {
            let channel = format!("mq:wake:thread:{}", thread_id.0);
            let _: Result<(), _> = redis::cmd("PUBLISH")
                .arg(&channel)
                .arg("1")
                .query_async(&mut conn)
                .await;
        }
    }

    async fn notify_worker(&self) {
        if let Ok(mut conn) = self.client.get_multiplexed_async_connection().await {
            let _: Result<(), _> = redis::cmd("PUBLISH")
                .arg("mq:wake:worker")
                .arg("1")
                .query_async(&mut conn)
                .await;
        }
    }
}

/// Local + optional Redis.
pub struct CompositeWake {
    local: mq_core::LocalWake,
    redis: Option<RedisWake>,
}

impl CompositeWake {
    pub fn new(local: mq_core::LocalWake, redis: Option<RedisWake>) -> Self {
        Self { local, redis }
    }

    pub fn local(&self) -> mq_core::LocalWake {
        self.local.clone()
    }
}

#[async_trait]
impl Wake for CompositeWake {
    async fn notify_thread(&self, thread_id: ThreadId) {
        self.local.notify_thread(thread_id).await;
        if let Some(r) = &self.redis {
            r.notify_thread(thread_id).await;
        }
    }

    async fn notify_worker(&self) {
        self.local.notify_worker().await;
        if let Some(r) = &self.redis {
            r.notify_worker().await;
        }
    }
}
