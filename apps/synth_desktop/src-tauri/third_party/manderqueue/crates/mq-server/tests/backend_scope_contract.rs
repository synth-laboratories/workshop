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
        let output = std::process::Command::new(backend_python(&root))
            .current_dir(&root)
            .env("PYTHONPATH", &root)
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

/// `MQ_TEST_BACKEND_PYTHON` overrides the interpreter (a shared venv with the
/// backend checkout on PYTHONPATH); default is the checkout's own `.venv`.
fn backend_python(root: &std::path::Path) -> std::path::PathBuf {
    std::env::var("MQ_TEST_BACKEND_PYTHON")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join(".venv/bin/python"))
}

/// Run the backend signing module with fixture keys only. Never production keys.
fn backend_issuer(root: &std::path::Path, key: &str, kid: &str, extra_jwks: &str, args: &[&str]) -> String {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let script = "import json, sys\n\
from services.mq.signing import mint_signed_mq_jwt, mint_grant_credential, public_jwks\n\
mode = sys.argv[1]\n\
if mode == 'jwks': print(json.dumps(public_jwks()))\n\
elif mode == 'owner': print(mint_signed_mq_jwt(principal={'kind':'human','id':'owner','org_id':'org'}, ttl_seconds=60))\n\
elif mode == 'grant': print(mint_grant_credential(json.loads(sys.argv[2])).token)\n";
    let output = std::process::Command::new(backend_python(root))
        .current_dir(root)
        .env("PYTHONPATH", root)
        .env("MQ_AUTH", "jwt").env("MQ_PROFILE", "deployed")
        .env("MQ_ISSUER_SIGNING_KEY_FILE", fixtures.join(key))
        .env("MQ_ISSUER_SIGNING_KID", kid)
        .env("MQ_ISSUER_ADDITIONAL_JWKS", std::fs::read_to_string(fixtures.join(extra_jwks)).unwrap())
        .args(["-c", script]).args(args)
        .output().expect("run backend fixture issuer");
    assert!(output.status.success(), "backend fixture issuer failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Run the backend delivery-bridge verifier against a live MQ listener.
fn backend_bridge_verify(root: &std::path::Path, mq_url: &str, recipient: &serde_json::Value, grant: &serde_json::Value, seq: u64) -> serde_json::Value {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let script = "import asyncio, json, sys\n\
from services.mq.delivery_grant import verify_delivery_grant, DeliveryGrantRefused\n\
recipient, grant, seq = json.loads(sys.argv[1]), json.loads(sys.argv[2]), int(sys.argv[3])\n\
try:\n    print(json.dumps({'ok': asyncio.run(verify_delivery_grant(recipient=recipient, grant=grant, message_seq=seq))}))\n\
except DeliveryGrantRefused as exc:\n    print(json.dumps({'refused': exc.code}))\n";
    let output = std::process::Command::new(backend_python(root))
        .current_dir(root)
        .env("PYTHONPATH", root)
        .env("MQ_ISSUER_SIGNING_KEY_FILE", fixtures.join("grant_k1.pem"))
        .env("MQ_ISSUER_SIGNING_KID", "fixture-k1")
        .env_remove("MQ_ISSUER_ADDITIONAL_JWKS")
        .env("MANDERQUEUE_HTTP_URL", mq_url)
        .args(["-c", script, &recipient.to_string(), &grant.to_string(), &seq.to_string()])
        .output().expect("run backend bridge verifier");
    assert!(output.status.success(), "bridge verifier failed: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("verifier JSON")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires MQ_TEST_BACKEND_ROOT (backend checkout with .venv or PYTHONPATH-able venv)"]
async fn backend_bridge_verifies_envelope_grants_against_live_mq() {
    let root = std::path::PathBuf::from(std::env::var("MQ_TEST_BACKEND_ROOT").expect("backend checkout required"));
    let published = backend_issuer(&root, "grant_k1.pem", "fixture-k1", "jwks_k2.json", &["jwks"]);
    let mut state = AppState::memory();
    state.auth = AuthMode::Keyset(std::sync::Arc::new(mq_server::Verifier::new(&published, None).unwrap()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mq_url = format!("http://{}", listener.local_addr().unwrap());
    let served = mq_server::router(state.clone());
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });

    let owner = Principal { kind: PrincipalKind::Human, org_id: "org".into(), id: "owner".into() };
    let thread = state.fabric.create_thread(&owner, CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() },
        title: None, participants: vec![Participant::new(owner.clone(), Role::Owner)], idempotency_key: None,
    }).await.unwrap().thread_id;
    let enrollment = state.fabric.enroll(&owner, mq_core::EnrollDevice { device_id: "dev".into(), session_id: "s".into(), label: None }).await.unwrap();
    let grant = state.fabric.create_grant(&owner, mq_core::CreateGrant {
        thread_id: thread, enrollment_id: enrollment.enrollment_id, operations: vec![mq_core::GrantOperation::Read],
        ttl_seconds: 3600, history_after_seq: None,
    }).await.unwrap();
    state.fabric.publish(&owner, thread, mq_core::PublishMessage { body: "for device".into(), ..Default::default() }).await.unwrap();
    let recipient = serde_json::to_value(&grant.principal).unwrap();
    let triple = |generation: u64, incarnation: u64| serde_json::json!({"grant_id": grant.grant_id, "generation": generation, "incarnation": incarnation});

    assert_eq!(backend_bridge_verify(&root, &mq_url, &recipient, &triple(0, 1), 1), serde_json::json!({"ok": triple(0, 1)}));
    assert_eq!(backend_bridge_verify(&root, &mq_url, &recipient, &triple(1, 1), 1), serde_json::json!({"refused": "grant_generation_stale"}));
    assert_eq!(backend_bridge_verify(&root, &mq_url, &recipient, &triple(0, 2), 1), serde_json::json!({"refused": "grant_incarnation_fenced"}));
    assert_eq!(backend_bridge_verify(&root, &mq_url, &recipient, &triple(0, 1), 0), serde_json::json!({"refused": "invalid_message_seq"}));
    state.fabric.revoke_grant(&owner, grant.grant_id).await.unwrap();
    assert_eq!(backend_bridge_verify(&root, &mq_url, &recipient, &triple(0, 1), 1), serde_json::json!({"refused": "grant_revoked"}));
    state.fabric.restore_grant(&owner, grant.grant_id).await.unwrap();
    assert_eq!(backend_bridge_verify(&root, &mq_url, &recipient, &triple(1, 1), 1), serde_json::json!({"ok": triple(1, 1)}));
    state.fabric.revoke_enrollment(&owner, enrollment.enrollment_id).await.unwrap();
    assert_eq!(backend_bridge_verify(&root, &mq_url, &recipient, &triple(2, 1), 1), serde_json::json!({"refused": "enrollment_revoked"}));
}

#[tokio::test]
#[ignore = "requires MQ_TEST_BACKEND_ROOT (backend checkout with .venv or PYTHONPATH-able venv)"]
async fn backend_signed_kid_grant_credentials_verify_and_rotate() {
    use http_body_util::BodyExt;
    let root = std::path::PathBuf::from(std::env::var("MQ_TEST_BACKEND_ROOT").expect("backend checkout required"));
    // MQ's keyset is exactly what the backend publishes (active k1 + overlap k2).
    let published = backend_issuer(&root, "grant_k1.pem", "fixture-k1", "jwks_k2.json", &["jwks"]);
    let verifier = std::sync::Arc::new(mq_server::Verifier::new(&published, None).expect("backend JWKS parses"));
    assert_eq!(verifier.kids(), vec!["fixture-k1".to_string(), "fixture-k2".to_string()]);
    let mut state = AppState::memory();
    state.auth = AuthMode::Keyset(verifier.clone());
    let call = |state: AppState, method: &'static str, uri: String, token: String, body: Option<serde_json::Value>| async move {
        let mut request = Request::builder().method(method).uri(uri).header("authorization", format!("Bearer {token}"));
        if body.is_some() { request = request.header("content-type", "application/json"); }
        let body = body.map(|b| Body::from(serde_json::to_vec(&b).unwrap())).unwrap_or_else(Body::empty);
        let response = mq_server::router(state).oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default())
    };
    let owner = backend_issuer(&root, "grant_k1.pem", "fixture-k1", "jwks_k2.json", &["owner"]);
    let (status, thread) = call(state.clone(), "POST", "/v1/threads".into(), owner.clone(), Some(serde_json::json!({
        "org_id":"org","scope":{"kind":"org","id":"org"},"title":null,
        "participants":[{"principal":{"kind":"human","id":"owner","org_id":"org"},"role":"owner"}]}))).await;
    assert_eq!(status, 201);
    let thread_id = thread["thread_id"].as_str().unwrap().to_string();
    let (_, enrollment) = call(state.clone(), "POST", "/v1/enrollments".into(), owner.clone(),
        Some(serde_json::json!({"device_id":"dev","session_id":"s"}))).await;
    let (status, grant) = call(state.clone(), "POST", "/v1/grants".into(), owner.clone(), Some(serde_json::json!({
        "thread_id":thread_id,"enrollment_id":enrollment["enrollment_id"],"operations":["read"],"ttl_seconds":3600}))).await;
    assert_eq!(status, 201);
    let grant_id = grant["grant_id"].as_str().unwrap();
    let (status, issuance) = call(state.clone(), "POST", format!("/v1/grants/{grant_id}/issuance"), owner.clone(),
        Some(serde_json::json!({"enrollment_id":enrollment["enrollment_id"],"incarnation":1}))).await;
    assert_eq!(status, 200);
    let k1 = backend_issuer(&root, "grant_k1.pem", "fixture-k1", "jwks_k2.json", &["grant", &issuance.to_string()]);
    let k2 = backend_issuer(&root, "grant_k2.pem", "fixture-k2", "jwks_k1.json", &["grant", &issuance.to_string()]);
    let history = format!("/v1/threads/{thread_id}/history");
    assert_eq!(call(state.clone(), "GET", history.clone(), k1.clone(), None).await.0, 200);
    assert_eq!(call(state.clone(), "GET", history.clone(), k2.clone(), None).await.0, 200);
    // Read-only grant cannot publish or reach other routes.
    let publish = Some(serde_json::json!({"kind":"notice","body":"x"}));
    assert_eq!(call(state.clone(), "POST", format!("/v1/threads/{thread_id}/messages"), k1.clone(), publish).await.0, 401);
    assert_eq!(call(state.clone(), "GET", "/v1/threads".into(), k1.clone(), None).await.0, 401);
    // Revocation is enforced on the backend-minted credential.
    assert_eq!(call(state.clone(), "POST", format!("/v1/grants/{grant_id}/revoke"), owner.clone(), Some(serde_json::json!({}))).await.0, 200);
    assert_eq!(call(state.clone(), "GET", history.clone(), k2.clone(), None).await, (403, serde_json::json!({"error":"grant_revoked"})));
    assert_eq!(call(state.clone(), "POST", format!("/v1/grants/{grant_id}/restore"), owner.clone(), Some(serde_json::json!({}))).await.0, 200);
    assert_eq!(call(state.clone(), "GET", history.clone(), k2.clone(), None).await.0, 403, "pre-revoke generation stays dead");
    let (_, issuance) = call(state.clone(), "POST", format!("/v1/grants/{grant_id}/issuance"), owner.clone(),
        Some(serde_json::json!({"enrollment_id":enrollment["enrollment_id"],"incarnation":1}))).await;
    let k1 = backend_issuer(&root, "grant_k1.pem", "fixture-k1", "jwks_k2.json", &["grant", &issuance.to_string()]);
    let k2 = backend_issuer(&root, "grant_k2.pem", "fixture-k2", "jwks_k1.json", &["grant", &issuance.to_string()]);
    assert_eq!(call(state.clone(), "GET", history.clone(), k1.clone(), None).await.0, 200);
    // Rotation completes: the backend now publishes only k2; the removed kid refuses.
    let rotated = backend_issuer(&root, "grant_k2.pem", "fixture-k2", "jwks_k2.json", &["jwks"]);
    verifier.replace_keys(&rotated).unwrap();
    assert_eq!(call(state.clone(), "GET", history.clone(), k1, None).await.0, 401);
    assert_eq!(call(state.clone(), "GET", history, k2, None).await.0, 200);
}
