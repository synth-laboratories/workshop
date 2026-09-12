use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use mq_core::{
    CreateThread, Participant, Principal, PrincipalKind, Role, ScopeBinding, ScopeKind, Wake,
};
use mq_server::{AppState, AuthMode};
use std::time::Duration;
use tower::ServiceExt;

async fn setup(state: &AppState) -> mq_core::ThreadId {
    let principal = Principal {
        kind: PrincipalKind::Human,
        org_id: "org".into(),
        id: "owner".into(),
    };
    state
        .fabric
        .create_thread(
            &principal,
            CreateThread {
                org_id: "org".into(),
                scope: ScopeBinding {
                    kind: ScopeKind::Org,
                    id: "org".into(),
                },
                title: None,
                participants: vec![Participant::new(principal.clone(), Role::Owner)],
                idempotency_key: None,
            },
        )
        .await
        .unwrap()
        .thread_id
}

#[tokio::test]
async fn scoped_token_is_enforced_on_reads_streams_and_global_routes() {
    let mut state = AppState::memory();
    let thread = setup(&state).await;
    let secret = "fixture-secret-at-least-32-bytes-long";
    state.auth = AuthMode::Jwt { secret: secret.into() };
    let claims = serde_json::json!({"iss":"manderqueue","aud":"manderqueue",
        "exp":chrono::Utc::now().timestamp()+60,"jti":"scope-fixture",
        "principal":{"kind":"human","id":"owner","org_id":"org"},
        "thread_scope":{"thread_id":thread.0,"operations":["read"]}});
    let token = jsonwebtoken::encode(&jsonwebtoken::Header::default(), &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes())).unwrap();
    for (path, status) in [
        (format!("/v1/threads/{}",thread.0), 200),
        (format!("/v1/threads/{}/messages",thread.0), 200),
        (format!("/v1/threads/{}/events",thread.0), 200),
        ("/v1/threads".into(), 401),
        (format!("/v1/threads/{}",uuid::Uuid::new_v4()), 401),
    ] {
        let response = mq_server::router(state.clone()).oneshot(Request::builder()
            .uri(path).header("authorization",format!("Bearer {token}"))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status().as_u16(), status);
    }
    let publish = mq_core::PublishMessage { body: "forbidden".into(), ..Default::default() };
    for (method, path, body) in [
        ("POST", format!("/v1/threads/{}/messages", thread.0), serde_json::to_value(publish).unwrap()),
        ("PATCH", format!("/v1/threads/{}/participants/human/owner", thread.0), serde_json::json!({"role":"observer"})),
    ] {
        let response = mq_server::router(state.clone()).oneshot(Request::builder()
            .method(method).uri(path).header("authorization",format!("Bearer {token}"))
            .header("content-type","application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status().as_u16(), 401);
    }
    let mut outsider = claims.clone();
    outsider["principal"]["id"] = serde_json::json!("not-a-member");
    let outsider_token = jsonwebtoken::encode(&jsonwebtoken::Header::default(), &outsider,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes())).unwrap();
    let response = mq_server::router(state).oneshot(Request::builder()
        .uri(format!("/v1/threads/{}/messages", thread.0))
        .header("authorization",format!("Bearer {outsider_token}"))
        .body(Body::empty()).unwrap()).await.unwrap();
    assert!(!response.status().is_success(), "signed scope cannot grant thread membership");
}

async fn stream(state: AppState, thread: mq_core::ThreadId, token: &str) -> Body {
    let response = mq_server::router(state)
        .oneshot(
            Request::builder()
                .uri(format!("/v1/threads/{}/events", thread.0))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.into_body()
}

#[tokio::test]
async fn persisted_revocation_closes_open_stream_and_refuses_future_access() {
    let state = AppState::memory();
    let thread = setup(&state).await;
    let owner = Principal { kind: PrincipalKind::Human, org_id: "org".into(), id: "owner".into() };
    let target = Principal { kind: PrincipalKind::Actor, org_id: "org".into(), id: "local-session".into() };
    state.fabric.add_participant(&owner, thread, Participant::new(target.clone(), Role::Agent)).await.unwrap();
    for body in ["claimed before revoke", "queued before revoke"] {
        state.fabric.publish(&owner, thread, mq_core::PublishMessage {
            body: body.into(), recipients: vec![target.clone()], ..Default::default()
        }).await.unwrap();
    }
    let claimed = state.fabric.claim_delivery_jobs(1).await.unwrap();
    assert_eq!(claimed.len(), 1);
    let mut body = stream(state.clone(), thread, "actor:org:local-session").await;
    let response = mq_server::router(state.clone()).oneshot(Request::builder()
        .method("PATCH").uri(format!("/v1/threads/{}/participants/actor/local-session", thread.0))
        .header("authorization", "Bearer human:org:owner").header("content-type", "application/json")
        .body(Body::from(r#"{"role":"revoked"}"#)).unwrap()).await.unwrap();
    assert_eq!(response.status(), 204);
    assert!(state.fabric.claim_delivery_jobs(10).await.unwrap().is_empty());
    assert!(state.fabric.settle_delivery_job(claimed[0].job_id, claimed[0].attempts, mq_core::DeliveryStatus::Delivered).await.is_err());
    let frame = tokio::time::timeout(Duration::from_secs(6), body.frame()).await.unwrap().unwrap().unwrap();
    assert!(std::str::from_utf8(frame.data_ref().unwrap()).unwrap().contains("revoked"));
    assert!(body.frame().await.is_none());
    assert!(state.fabric.read_messages(&target, thread, 0, 10).await.is_err());
    assert!(state.fabric.publish(&target, thread, mq_core::PublishMessage { body: "stale authority".into(), ..Default::default() }).await.is_err());
    assert!(state.fabric.set_participant_role(&target, thread, &target, Role::Agent).await.is_err());
    assert!(state.fabric.set_participant_role(&owner, thread, &owner, Role::Revoked).await.is_err());
    state.fabric.set_participant_role(&owner, thread, &target, Role::Agent).await.unwrap();
    assert!(state.fabric.read_messages(&target, thread, 0, 10).await.is_ok());
    assert!(state.fabric.claim_delivery_jobs(10).await.unwrap().is_empty(), "restoration must not revive cancelled jobs");
}

#[tokio::test]
async fn lag_is_explicit_and_stream_recovers() {
    let state = AppState::memory();
    let thread = setup(&state).await;
    let mut body = stream(state.clone(), thread, "human:org:owner").await;
    for _ in 0..300 {
        state.local_wake.notify_thread(thread).await;
    }
    let frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(std::str::from_utf8(frame.data_ref().unwrap())
        .unwrap()
        .contains("event: resync"));
    let frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(std::str::from_utf8(frame.data_ref().unwrap())
        .unwrap()
        .contains("event: thread_wake"));
}

#[tokio::test]
async fn expired_credentials_close_an_existing_stream_without_exposing_wakes() {
    let secret = "fixture-secret-with-at-least-thirty-two-bytes";
    let mut state = AppState::memory();
    state.auth = AuthMode::Jwt {
        secret: secret.into(),
    };
    let thread = setup(&state).await;
    let claims = serde_json::json!({"principal": {"kind": "human", "id": "owner", "org_id": "org"},
        "aud": "manderqueue", "iss": "manderqueue", "jti": "fixture",
        "exp": chrono::Utc::now().timestamp() + 2});
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();
    let mut body = stream(state.clone(), thread, &token).await;
    let mut quiet_body = stream(state.clone(), thread, &token).await;
    let quiet = tokio::spawn(async move {
        let frame = tokio::time::timeout(Duration::from_secs(7), quiet_body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(std::str::from_utf8(frame.data_ref().unwrap())
            .unwrap()
            .contains("event: revoked"));
        assert!(quiet_body.frame().await.is_none());
    });
    tokio::time::sleep(Duration::from_secs(2)).await;
    let frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = std::str::from_utf8(frame.data_ref().unwrap()).unwrap();
    assert!(text.contains("event: revoked"));
    assert!(!text.contains("thread_wake"));
    assert!(body.frame().await.is_none());
    quiet.await.unwrap();
}
