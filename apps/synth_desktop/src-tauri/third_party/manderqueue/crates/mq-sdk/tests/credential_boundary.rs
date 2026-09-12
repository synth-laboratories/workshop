use axum::{response::Redirect, routing::get, Router};
use mq_core::ThreadId;
use mq_sdk::{MqClient, SdkError};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::net::TcpListener;

#[tokio::test]
async fn authenticated_reads_do_not_follow_even_same_origin_redirects() {
    let hits = Arc::new(AtomicUsize::new(0));
    let observed = hits.clone();
    let app = Router::new()
        .route(
            "/v1/threads/{id}/messages",
            get(|| async { Redirect::temporary("/credential-trap") }),
        )
        .route(
            "/credential-trap",
            get(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                async { axum::Json(Vec::<serde_json::Value>::new()) }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = MqClient::new(format!("http://{address}"), "fixture-device-credential");
    let error = client
        .read_messages(ThreadId::new(), 0, 1)
        .await
        .unwrap_err();
    assert!(matches!(error, SdkError::Api { status, .. } if status.as_u16() == 307));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    server.abort();
}
