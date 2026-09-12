//! Opt-in issuer/validator conformance using the real backend Python issuer.
use axum::{body::Body, http::Request};
use mq_core::{CreateThread, Participant, Principal, PrincipalKind, Role, ScopeBinding, ScopeKind};
use mq_server::{AppState, AuthMode};
use tower::ServiceExt;

#[tokio::test]
#[ignore = "requires MQ_TEST_BACKEND_ROOT with its Python .venv"]
async fn backend_scoped_token_enforces_http_permissions() {
    let root = std::path::PathBuf::from(std::env::var("MQ_TEST_BACKEND_ROOT").expect("backend checkout required"));
    let secret = "interop-fixture-key-never-a-production-secret";
    let mut state = AppState::memory();
    state.auth = AuthMode::Jwt { secret: secret.into() };
    let principal = Principal { kind: PrincipalKind::Human, org_id: "org".into(), id: "owner".into() };
    let thread = state.fabric.create_thread(&principal, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() },
        title: None, participants: vec![Participant::new(principal.clone(), Role::Owner)], idempotency_key: None,
    }).await.unwrap().thread_id;
    for (operation, publish_status, read_status) in [("read", 401, 200), ("publish", 201, 401)] {
        let output = std::process::Command::new(root.join(".venv/bin/python"))
            .current_dir(&root)
            .env("MQ_AUTH", "jwt").env("MQ_PROFILE", "deployed").env("MQ_JWT_SECRET", secret)
            .args(["-c", "import sys; from services.mq.jwt_mint import mint_mq_thread_bearer; print(mint_mq_thread_bearer(kind='human',org_id='org',principal_id='owner',thread_id=sys.argv[1],operations=(sys.argv[2],),grant_generation=0))", &thread.0.to_string(), operation])
            .output().expect("run backend fixture issuer");
        assert!(output.status.success(), "backend fixture issuer failed");
        let token = String::from_utf8(output.stdout).unwrap();
        let authorization = format!("Bearer {}", token.trim());
        for (path, expected) in [
            (format!("/v1/threads/{}/messages", thread.0), read_status),
            (format!("/v1/threads/{}", uuid::Uuid::new_v4()), 401),
            ("/v1/threads".into(), 401),
        ] {
            let response = mq_server::router(state.clone()).oneshot(Request::builder().uri(path)
                .header("authorization", &authorization).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status().as_u16(), expected);
        }
        let body = mq_core::PublishMessage { body: "issuer conformance".into(), ..Default::default() };
        let response = mq_server::router(state.clone()).oneshot(Request::builder()
            .method("POST").uri(format!("/v1/threads/{}/messages", thread.0))
            .header("authorization", &authorization).header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status().as_u16(), publish_status);
    }
    assert_eq!(state.fabric.read_messages(&principal, thread, 0, 10).await.unwrap().len(), 1);
}
