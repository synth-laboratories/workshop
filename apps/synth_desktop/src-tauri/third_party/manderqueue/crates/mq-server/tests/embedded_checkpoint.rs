use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use mq_core::{MemoryCheckpoint, MemoryStore};
use tower::ServiceExt;

#[tokio::test]
async fn concurrent_snapshots_do_not_split_message_and_delivery_job() {
    use mq_core::*;
    use std::sync::Arc;
    let store = MemoryStore::default();
    let fabric = Fabric::from_store(Arc::new(store.clone()));
    let owner = Principal {
        kind: PrincipalKind::System,
        org_id: "cut".into(),
        id: "owner".into(),
    };
    let member = Principal {
        kind: PrincipalKind::Actor,
        org_id: "cut".into(),
        id: "reader".into(),
    };
    let thread = fabric
        .create_thread(
            &owner,
            CreateThread {
                org_id: "cut".into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Project,
                    id: "cut".into(),
                },
                title: None,
                idempotency_key: None,
                participants: vec![
                    Participant::new(owner.clone(), Role::Owner),
                    Participant::new(member, Role::Agent),
                ],
            },
        )
        .await
        .unwrap();
    let token = "b".repeat(64);
    let app = mq_server::embedded::embedded_router(store.clone(), token.clone());
    let mut writes = Vec::new();
    for i in 0..40 {
        let app = app.clone();
        writes.push(tokio::spawn(async move {
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/v1/threads/{}/messages", thread.thread_id.0))
                        .header("authorization", "Bearer system:cut:owner")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::to_vec(&PublishMessage {
                                body: format!("message {i}"),
                                idempotency_key: Some(format!("cut-{i}")),
                                ..Default::default()
                            })
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(response.status().is_success());
        }));
    }
    for _ in 0..20 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/_embedded/checkpoint")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let snapshot: MemoryCheckpoint = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            snapshot.jobs.len(),
            snapshot.threads[0].2.len(),
            "checkpoint cut through append/enqueue"
        );
        MemoryStore::from_checkpoint(snapshot).unwrap();
        tokio::task::yield_now().await;
    }
    for write in writes {
        write.await.unwrap();
    }
    assert_eq!(store.checkpoint().jobs.len(), 40);
}

#[tokio::test]
async fn checkpoint_is_embedded_only_and_requires_control_token() {
    let token = "a".repeat(64);
    let app = mq_server::embedded::embedded_router(MemoryStore::default(), token.clone());
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/_embedded/checkpoint")
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(request()).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        mq_server::app().oneshot(request()).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/_embedded/checkpoint")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let snapshot: MemoryCheckpoint = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(snapshot.version, 2);
    assert!(snapshot.threads.is_empty());
}
