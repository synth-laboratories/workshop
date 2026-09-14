mod auth;
pub mod delivery;
mod error;
pub mod embedded;
pub mod postgres;
pub mod redis_wake;
mod routes;
pub mod worker;
pub mod write_buffer;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use mq_core::{BatchingStore, Fabric, LocalWake, Wake};

pub use auth::{AuthMode, Verifier, AUDIENCE, LEGACY_ISSUER, SIGNED_ISSUER};
pub use routes::app;

#[derive(Clone)]
pub struct AppState {
    pub fabric: Fabric,
    pub local_wake: LocalWake,
    pub database_url: Option<String>,
    pub auth: AuthMode,
}

impl AppState {
    pub fn memory() -> Self {
        let local_wake = LocalWake::new(256);
        let fabric = Fabric::memory().with_wake(Arc::new(local_wake.clone()) as Arc<dyn Wake>);
        Self {
            fabric,
            local_wake,
            database_url: None,
            auth: AuthMode::Dev,
        }
    }

    pub fn from_parts(
        fabric: Fabric,
        local_wake: LocalWake,
        database_url: Option<String>,
        auth: AuthMode,
    ) -> Self {
        Self {
            fabric,
            local_wake,
            database_url,
            auth,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::memory()
    }
}

pub fn router(state: AppState) -> Router {
    routes::router(state)
}

pub struct Boot {
    pub fabric: Fabric,
    pub local_wake: LocalWake,
    pub database_url: Option<String>,
    pub write_buffer: String,
    pub auth: AuthMode,
}

/// Poll the configured JWKS file so key rotation needs no restart.
/// `MQ_JWT_JWKS_RELOAD_SECS` (default 30; 0 disables). Inline keysets never reload.
fn spawn_jwks_reload(auth: &AuthMode) {
    let AuthMode::Keyset(verifier) = auth else { return };
    if !verifier.has_file_source() {
        return;
    }
    let secs: u64 = std::env::var("MQ_JWT_JWKS_RELOAD_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    if secs == 0 {
        return;
    }
    let verifier = verifier.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(secs));
        tick.tick().await;
        loop {
            tick.tick().await;
            match verifier.reload_if_changed() {
                Ok(true) => eprintln!("mq jwks reloaded kids={:?}", verifier.kids()),
                Ok(false) => {}
                // Errors never include file contents; current keys stay active.
                Err(error) => eprintln!("mq jwks reload refused, keeping current keys: {error}"),
            }
        }
    });
}

pub async fn boot_from_env() -> Result<Boot, Box<dyn std::error::Error + Send + Sync>> {
    let auth = AuthMode::from_env()?;
    spawn_jwks_reload(&auth);
    let profile = std::env::var("MQ_PROFILE").unwrap_or_else(|_| "deployed".into());
    let database_url = std::env::var("DATABASE_URL").ok().filter(|s| !s.trim().is_empty());
    let configured_buffer = std::env::var("MQ_WRITE_BUFFER").unwrap_or_else(|_| "off".into());
    validate_profile(&profile, database_url.is_some(), &configured_buffer)?;
    let local_wake = LocalWake::new(1024);
    let redis_url = std::env::var("REDIS_URL").ok().filter(|s| !s.is_empty());
    let redis = match redis_url.as_deref() {
        Some(url) => match redis_wake::RedisWake::connect(url) {
            Ok(r) => {
                let _ = r.pipe_into_local(local_wake.clone()).await;
                Some(r)
            }
            Err(e) => {
                eprintln!("redis connect failed (degraded poll-only): {e}");
                None
            }
        },
        None => None,
    };

    let wake: Arc<dyn Wake> = Arc::new(redis_wake::CompositeWake::new(
        local_wake.clone(),
        redis,
    ));

    let write_buffer = std::env::var("MQ_WRITE_BUFFER")
        .unwrap_or_else(|_| "off".into())
        .to_lowercase();
    let batch_size: usize = std::env::var("MQ_WRITE_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);
    let flush_ms: u64 = std::env::var("MQ_WRITE_FLUSH_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(25);

    let database_url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty());
    let fabric = match &database_url {
        Some(url) => {
            let store = postgres::PostgresStore::connect(url).await?;
            match write_buffer.as_str() {
                "memory" | "redis" => {
                    if write_buffer == "redis" && redis_url.is_none() {
                        return Err("MQ_WRITE_BUFFER=redis requires REDIS_URL".into());
                    }
                    let durable: Arc<dyn mq_core::Store> = Arc::new(store);
                    let mut batching = BatchingStore::new(durable.clone());
                    if write_buffer == "redis" {
                        if let Some(rurl) = redis_url.as_deref() {
                            let buffer = write_buffer::RedisWriteBuffer::connect(rurl)?;
                            let recovered = buffer.drain(10_000).await.unwrap_or_default();
                            if !recovered.is_empty() {
                                eprintln!(
                                    "mq redis write buffer recovering {} staged publishes",
                                    recovered.len()
                                );
                                durable.flush_write_batch(&recovered).await?;
                            }
                            batching = batching.with_mirror(Arc::new(buffer));
                        }
                    }
                    let batching = Arc::new(batching);
                    let flusher = batching.clone();
                    tokio::spawn(async move {
                        let mut tick =
                            tokio::time::interval(Duration::from_millis(flush_ms.max(1)));
                        loop {
                            tick.tick().await;
                            if let Err(e) = flusher.flush(batch_size.max(1)).await {
                                eprintln!("mq write buffer flush failed: {e:?}");
                            }
                        }
                    });
                    Fabric::from_store(batching).with_wake(wake)
                }
                _ => Fabric::from_store(Arc::new(store)).with_wake(wake),
            }
        }
        None => Fabric::memory().with_wake(wake),
    };

    Ok(Boot {
        fabric,
        local_wake,
        database_url,
        write_buffer,
        auth,
    })
}

pub async fn fabric_from_env() -> Result<Fabric, Box<dyn std::error::Error + Send + Sync>> {
    Ok(boot_from_env().await?.fabric)
}

/// Reject unsafe storage before connecting infrastructure. See docs/DELIVERY_SECURITY.md.
pub fn validate_profile(profile: &str, has_database: bool, buffer: &str) -> Result<(), &'static str> {
    if !matches!(profile, "local" | "deployed") { return Err("MQ_PROFILE must be local or deployed"); }
    if !matches!(buffer, "off" | "memory" | "redis") { return Err("unknown MQ_WRITE_BUFFER"); }
    if profile == "deployed" && (!has_database || buffer != "off") {
        return Err("deployed MQ requires DATABASE_URL and MQ_WRITE_BUFFER=off");
    }
    Ok(())
}

#[cfg(test)]
mod profile_tests {
    #[test]
    fn deployed_requires_durability() {
        assert!(super::validate_profile("deployed", false, "off").is_err());
        assert!(super::validate_profile("deployed", true, "memory").is_err());
        assert!(super::validate_profile("deployed", true, "off").is_ok());
        assert!(super::validate_profile("local", false, "off").is_ok());
        assert!(super::validate_profile("typo", true, "off").is_err());
    }
}
