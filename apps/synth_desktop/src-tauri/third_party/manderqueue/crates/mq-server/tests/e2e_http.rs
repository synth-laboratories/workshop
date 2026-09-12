//! HTTP end-to-end: Effort judgment ask → answer over the OpenAPI-shaped surface.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use mq_core::{CreateThread, Message, MessageKind, Participant, PrincipalKind, PublishMessage, ScopeKind, Thread};
use serde_json::json;
use tower::ServiceExt;

fn bearer(kind: &str, org: &str, id: &str) -> String {
    format!("Bearer {kind}:{org}:{id}")
}

async fn json_body<T: serde::de::DeserializeOwned>(res: axum::response::Response) -> T {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn health_ok() {
    let app = mq_server::app();
    let res = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn unauthenticated_create_is_401() {
    let app = mq_server::app();
    let body = json!({
        "org_id": "org-1",
        "scope": { "kind": "effort", "id": "e1" },
        "participants": []
    });
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/threads")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e2e_effort_judgment_ask_answer() {
    let app = mq_server::app();
    let org = "org-1";
    let async_auth = bearer("intern_async", org, "intern-a");
    let human_auth = bearer("human", org, "user-1");

    let create = CreateThread {
        org_id: org.into(),
        scope: mq_core::ScopeBinding {
            kind: ScopeKind::Effort,
            id: "effort-craftax".into(),
        },
        title: Some("metric".into()),
        participants: vec![
            Participant::new(
                mq_core::Principal {
                    kind: PrincipalKind::InternAsync,
                    id: "intern-a".into(),
                    org_id: org.into(),
                },
                mq_core::Role::Owner,
            ),
            Participant::new(
                mq_core::Principal {
                    kind: PrincipalKind::Human,
                    id: "user-1".into(),
                    org_id: org.into(),
                },
                mq_core::Role::Member,
            ),
        ],
        idempotency_key: None,
    };

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/threads")
                .header("authorization", &async_auth)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&create).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let thread: Thread = json_body(res).await;
    assert_eq!(thread.scope.kind, ScopeKind::Effort);

    let ask = PublishMessage {
        kind: MessageKind::Ask,
        body: "which metric?".into(),
        idempotency_key: Some("ask-e2e".into()),
        ..Default::default()
    };
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/threads/{}/messages", thread.thread_id.0))
                .header("authorization", &async_auth)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&ask).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let ask_msg: Message = json_body(res).await;

    let answer = PublishMessage {
        kind: MessageKind::Answer,
        body: "dense + success".into(),
        correlation_id: Some(ask_msg.message_id.0.to_string()),
        idempotency_key: Some("ans-e2e".into()),
        ..Default::default()
    };
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/threads/{}/messages", thread.thread_id.0))
                .header("authorization", &human_auth)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&answer).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let res = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/threads/{}/messages?after_seq=0&limit=10",
                    thread.thread_id.0
                ))
                .header("authorization", &async_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let page: Vec<Message> = json_body(res).await;
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].kind, MessageKind::Ask);
    assert_eq!(page[1].kind, MessageKind::Answer);
}

#[tokio::test]
async fn e2e_non_member_forbidden() {
    let app = mq_server::app();
    let org = "org-1";
    let async_auth = bearer("intern_async", org, "intern-a");
    let stranger = bearer("human", org, "nosy");

    let create = CreateThread {
        org_id: org.into(),
        scope: mq_core::ScopeBinding {
            kind: ScopeKind::Effort,
            id: "e1".into(),
        },
        title: None,
        participants: vec![Participant::new(
            mq_core::Principal {
                kind: PrincipalKind::InternAsync,
                id: "intern-a".into(),
                org_id: org.into(),
            },
            mq_core::Role::Owner,
        )],
        idempotency_key: None,
    };

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/threads")
                .header("authorization", &async_auth)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&create).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let thread: Thread = json_body(res).await;

    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/threads/{}/messages", thread.thread_id.0))
                .header("authorization", &stranger)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn control_plane_routes_do_not_exist() {
    let app = mq_server::app();
    for path in ["/ensure", "/v1/ensure", "/pause", "/v1/pause", "/budget"] {
        let res = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::NOT_FOUND,
            "{path} must not be an MQ control-plane route"
        );
    }
}

#[tokio::test]
async fn e2e_ensure_thread_idempotent() {
    let app = mq_server::app();
    let org = "org-1";
    let auth = bearer("intern_async", org, "intern-a");

    let body = CreateThread {
        org_id: org.into(),
        scope: mq_core::ScopeBinding {
            kind: ScopeKind::Effort,
            id: "e-ensure".into(),
        },
        title: Some("ensure".into()),
        participants: vec![Participant::new(
            mq_core::Principal {
                kind: PrincipalKind::InternAsync,
                id: "intern-a".into(),
                org_id: org.into(),
            },
            mq_core::Role::Owner,
        )],
        idempotency_key: Some("ensure-key-1".into()),
    };

    let res1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/threads/ensure")
                .header("authorization", &auth)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res1.status(), StatusCode::OK);
    let t1: Thread = json_body(res1).await;

    let res2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/threads/ensure")
                .header("authorization", &auth)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let t2: Thread = json_body(res2).await;
    assert_eq!(t1.thread_id, t2.thread_id);
}
