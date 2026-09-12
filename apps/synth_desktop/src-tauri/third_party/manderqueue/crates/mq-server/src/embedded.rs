//! Isolated container mode: local transport, no workers or external backends.
use std::sync::Arc;
use axum::{extract::Request, http::{HeaderMap, StatusCode}, middleware::{self, Next}, response::Response, routing::post, Json, Router};
use mq_core::{Fabric, LocalWake, MemoryStore};
use tokio::sync::RwLock;
use crate::{AppState, AuthMode};

pub fn embedded_router(store: MemoryStore, token: String) -> Router {
    let barrier = Arc::new(RwLock::new(()));
    let checkpoint_barrier = barrier.clone();
    let checkpoint_store = store.clone();
    let wake = LocalWake::new(256);
    let fabric = Fabric::from_store(Arc::new(store)).with_wake(Arc::new(wake.clone()));
    crate::router(AppState::from_parts(fabric, wake, None, AuthMode::Dev))
        .route("/_embedded/checkpoint", post(move |headers: HeaderMap| {
            let barrier = checkpoint_barrier.clone();
            let store = checkpoint_store.clone();
            let expected = format!("Bearer {token}");
            async move {
                if headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(expected.as_str()) {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                // Drain complete API operations, not merely individual store
                // writes: append + enqueue must lie on the same side of the cut.
                let _guard = barrier.write().await;
                Ok(Json(store.checkpoint()))
            }
        }))
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let barrier = barrier.clone();
            async move {
                if req.uri().path() == "/_embedded/checkpoint" { return next.run(req).await; }
                let _guard = barrier.read().await;
                let response: Response = next.run(req).await;
                response
            }
        }))
}

pub async fn serve() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var("MQ_PROFILE").as_deref() != Ok("local") {
        return Err("embedded mode requires explicit MQ_PROFILE=local".into());
    }
    for key in ["DATABASE_URL", "REDIS_URL", "MQ_BRIDGE_BASE_URL"] {
        if std::env::var(key).is_ok_and(|s| !s.is_empty()) { return Err(format!("embedded mode forbids {key}").into()); }
    }
    if std::env::var("MQ_WRITE_BUFFER").is_ok_and(|v| v != "off") { return Err("embedded mode requires synchronous writes".into()); }
    let token = std::env::var("MQ_CHECKPOINT_TOKEN")?;
    if token.len() < 32 { return Err("checkpoint token must have at least 32 characters".into()); }
    let bind: std::net::SocketAddr = std::env::var("MQ_BIND").unwrap_or_else(|_| "127.0.0.1:8088".into()).parse()?;
    if !bind.ip().is_loopback() { return Err("embedded mode binds loopback only".into()); }
    let store = match std::env::var("MQ_RESTORE_FILE") {
        Ok(path) => MemoryStore::from_checkpoint(serde_json::from_slice(&std::fs::read(path)?)?)?,
        Err(_) => MemoryStore::default(),
    };
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, embedded_router(store, token)).await?;
    Ok(())
}
