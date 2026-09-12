//! Redis list write buffer → batch flush into [`BatchingStore`]-style durable path.
//!
//! Hot publishes are also mirrored here so a flusher (or crash recovery) can
//! drain `mq:writebuf` into Postgres with one txn per batch.

use mq_core::{BufferedPublish, PublishMirror};
use redis::AsyncCommands;

const KEY: &str = "mq:writebuf";

#[derive(Clone)]
pub struct RedisWriteBuffer {
    client: redis::Client,
}

#[async_trait::async_trait]
impl PublishMirror for RedisWriteBuffer {
    async fn mirror(&self, item: &BufferedPublish) {
        if let Err(e) = self.push(item).await {
            eprintln!("mq redis write buffer mirror failed: {e}");
        }
    }
}

impl RedisWriteBuffer {
    pub fn connect(redis_url: &str) -> Result<Self, redis::RedisError> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
        })
    }

    async fn conn(&self) -> Result<redis::aio::MultiplexedConnection, redis::RedisError> {
        self.client.get_multiplexed_async_connection().await
    }

    pub async fn push(&self, item: &BufferedPublish) -> Result<(), redis::RedisError> {
        let mut conn = self.conn().await?;
        let payload = serde_json::to_string(item).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "serialize BufferedPublish",
                e.to_string(),
            ))
        })?;
        let _: () = conn.rpush(KEY, payload).await?;
        Ok(())
    }

    pub async fn len(&self) -> Result<usize, redis::RedisError> {
        let mut conn = self.conn().await?;
        let n: usize = conn.llen(KEY).await?;
        Ok(n)
    }

    /// Atomically take up to `max` items from the head of the buffer.
    pub async fn drain(&self, max: usize) -> Result<Vec<BufferedPublish>, redis::RedisError> {
        if max == 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.conn().await?;
        // LRANGE 0..max-1 then LTRIM max..-1 — single-flusher assumption.
        let raw: Vec<String> = redis::cmd("LRANGE")
            .arg(KEY)
            .arg(0)
            .arg(max as isize - 1)
            .query_async(&mut conn)
            .await?;
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        let _: () = redis::cmd("LTRIM")
            .arg(KEY)
            .arg(raw.len() as isize)
            .arg(-1)
            .query_async(&mut conn)
            .await?;
        let mut out = Vec::with_capacity(raw.len());
        for s in raw {
            let item: BufferedPublish = serde_json::from_str(&s).map_err(|e| {
                redis::RedisError::from((
                    redis::ErrorKind::TypeError,
                    "deserialize BufferedPublish",
                    e.to_string(),
                ))
            })?;
            out.push(item);
        }
        Ok(out)
    }

    pub async fn clear(&self) -> Result<(), redis::RedisError> {
        let mut conn = self.conn().await?;
        let _: () = conn.del(KEY).await?;
        Ok(())
    }
}

/// Background flusher: drain Redis buffer → `flush_write_batch` on durable store.
pub async fn run_redis_flusher(
    buffer: RedisWriteBuffer,
    durable: std::sync::Arc<dyn mq_core::Store>,
    batch_size: usize,
    flush_ms: u64,
) {
    let mut interval =
        tokio::time::interval(std::time::Duration::from_millis(flush_ms.max(1)));
    loop {
        interval.tick().await;
        match buffer.drain(batch_size.max(1)).await {
            Ok(batch) if !batch.is_empty() => {
                if let Err(e) = durable.flush_write_batch(&batch).await {
                    eprintln!("mq write buffer flush failed: {e:?}");
                    // Re-queue best-effort so we don't drop.
                    for item in batch.into_iter().rev() {
                        let _ = buffer.push(&item).await;
                    }
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("mq write buffer drain failed: {e}"),
        }
    }
}
