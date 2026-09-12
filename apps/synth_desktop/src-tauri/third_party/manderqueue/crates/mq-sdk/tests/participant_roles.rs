use axum::{extract::Path, http::{HeaderMap, StatusCode}, routing::patch, Json, Router};
use mq_core::{PrincipalKind, Role, ThreadId};
use mq_sdk::{MqClient, SdkError};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

#[tokio::test]
async fn role_change_encodes_identity_and_preserves_refusal_without_retry() {
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let app = Router::new().route("/v1/threads/{thread}/participants/{kind}/{id}", patch(
        move |Path((_thread, kind, id)): Path<(String,String,String)>, headers: HeaderMap, Json(body): Json<serde_json::Value>| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move {
                assert_eq!(kind, "actor");
                assert_eq!(id, "device/session?epoch=2#fragment");
                assert_eq!(headers["authorization"], "Bearer fixture");
                assert_eq!(body, serde_json::json!({"role":"revoked"}));
                StatusCode::FORBIDDEN
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener,app).await.unwrap(); });
    let client = MqClient::new(format!("http://{address}"), "fixture");
    let error = client.set_participant_role(ThreadId::new(), PrincipalKind::Actor,
        "device/session?epoch=2#fragment", Role::Revoked).await.unwrap_err();
    assert!(matches!(error, SdkError::Api { status, .. } if status.as_u16()==403));
    assert_eq!(count.load(Ordering::SeqCst),1);
    assert!(client.set_participant_role(ThreadId::new(),PrincipalKind::Actor,"..",Role::Revoked).await.is_err());
    assert_eq!(count.load(Ordering::SeqCst),1);
    server.abort();
}
