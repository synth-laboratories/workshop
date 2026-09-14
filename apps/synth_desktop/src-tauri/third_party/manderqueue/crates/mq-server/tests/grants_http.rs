//! Grant credentials end to end over HTTP with EdDSA/kid verification.
//! Fixture keys only (tests/fixtures); see docs/WORKSHOP_GRANT_CONTRACT.md.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{body::Body, http::Request};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use mq_core::{Grant, Principal, PrincipalKind};
use mq_server::{AppState, AuthMode, Verifier, AUDIENCE, LEGACY_ISSUER, SIGNED_ISSUER};
use serde_json::{json, Value};
use tower::ServiceExt;

const K1_PEM: &str = include_str!("fixtures/grant_k1.pem");
const K2_PEM: &str = include_str!("fixtures/grant_k2.pem");
const K1_JWKS: &str = include_str!("fixtures/jwks_k1.json");
const K2_JWKS: &str = include_str!("fixtures/jwks_k2.json");
const LEGACY: &str = "fixture-secret-at-least-32-bytes-long";

fn both_jwks() -> String {
    let mut k1: Value = serde_json::from_str(K1_JWKS).unwrap();
    let k2: Value = serde_json::from_str(K2_JWKS).unwrap();
    k1["keys"].as_array_mut().unwrap().push(k2["keys"][0].clone());
    k1.to_string()
}

struct Harness {
    state: AppState,
    verifier: Arc<Verifier>,
    clock: Arc<Mutex<DateTime<Utc>>>,
    thread: String,
}

fn sign(pem: &str, kid: &str, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some(kid.into());
    encode(&header, claims, &EncodingKey::from_ed_pem(pem.as_bytes()).unwrap()).unwrap()
}

fn owner_token(pem: &str, kid: &str, org: &str, id: &str) -> String {
    let now = Utc::now().timestamp();
    sign(pem, kid, &json!({"iss":SIGNED_ISSUER,"aud":AUDIENCE,"iat":now,"exp":now+60,
        "jti":uuid::Uuid::new_v4(),"principal":{"kind":"human","id":id,"org_id":org}}))
}

/// Mirrors the backend issuer: every authority field comes from the issuance response.
fn grant_claims(grant: &Value, operations: Value) -> Value {
    let now = Utc::now().timestamp();
    json!({"iss":SIGNED_ISSUER,"aud":AUDIENCE,"iat":now,"exp":now+300,"jti":uuid::Uuid::new_v4(),
        "principal":grant["principal"],
        "grant":{"grant_id":grant["grant_id"],"thread_id":grant["thread_id"],"enrollment_id":grant["enrollment_id"],
            "operations":operations,"generation":grant["generation"],"incarnation":grant["incarnation"]}})
}

fn grant_token(pem: &str, kid: &str, grant: &Value) -> String {
    sign(pem, kid, &grant_claims(grant, grant["operations"].clone()))
}

impl Harness {
    async fn new() -> Self {
        let verifier = Arc::new(Verifier::new(&both_jwks(), Some(LEGACY.into())).unwrap());
        let clock = Arc::new(Mutex::new(Utc::now()));
        let source = clock.clone();
        let mut state = AppState::memory();
        state.fabric = state.fabric.clone().with_clock(Arc::new(move || *source.lock().unwrap()));
        state.auth = AuthMode::Keyset(verifier.clone());
        let mut h = Self { state, verifier, clock, thread: String::new() };
        let owner = h.owner();
        let (status, thread) = h.call("POST", "/v1/threads", &owner, Some(json!({
            "org_id":"org","scope":{"kind":"org","id":"org"},"title":null,
            "participants":[{"principal":{"kind":"human","id":"owner","org_id":"org"},"role":"owner"},
                            {"principal":{"kind":"human","id":"member","org_id":"org"},"role":"member"}]}))).await;
        assert_eq!(status, 201);
        h.thread = thread["thread_id"].as_str().unwrap().to_string();
        h
    }

    fn owner(&self) -> String {
        owner_token(K1_PEM, "fixture-k1", "org", "owner")
    }

    async fn call(&self, method: &str, uri: &str, token: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = Request::builder().method(method).uri(uri).header("authorization", format!("Bearer {token}"));
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(serde_json::to_vec(&body).unwrap())
            }
            None => Body::empty(),
        };
        let response = mq_server::router(self.state.clone()).oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn publish(&self, token: &str, body: &str) -> (u16, Value) {
        self.call("POST", &format!("/v1/threads/{}/messages", self.thread), token, Some(json!({"kind":"notice","body":body}))).await
    }

    async fn history(&self, token: &str, after: u64) -> (u16, Value) {
        self.call("GET", &format!("/v1/threads/{}/history?after_seq={after}", self.thread), token, None).await
    }

    async fn enroll(&self, token: &str, device: &str) -> Value {
        let (status, enrollment) = self.call("POST", "/v1/enrollments", token, Some(json!({"device_id":device,"session_id":"s1"}))).await;
        assert_eq!(status, 201, "{enrollment}");
        enrollment
    }

    async fn grant(&self, enrollment: &Value, operations: Value) -> Value {
        let (status, grant) = self.call("POST", "/v1/grants", &self.owner(), Some(json!({
            "thread_id":self.thread,"enrollment_id":enrollment["enrollment_id"],"operations":operations,"ttl_seconds":3600}))).await;
        assert_eq!(status, 201, "{grant}");
        grant
    }

    async fn issuance(&self, token: &str, grant: &Value, incarnation: u64) -> (u16, Value) {
        self.call("POST", &format!("/v1/grants/{}/issuance", grant["grant_id"].as_str().unwrap()), token,
            Some(json!({"enrollment_id":grant["enrollment_id"],"incarnation":incarnation}))).await
    }

    async fn admin(&self, grant: &Value, action: &str, body: Option<Value>) -> (u16, Value) {
        self.call("POST", &format!("/v1/grants/{}/{action}", grant["grant_id"].as_str().unwrap()), &self.owner(), body.or(Some(json!({})))).await
    }
}

#[tokio::test]
async fn history_bounds_operations_and_route_scoping() {
    let h = Harness::new().await;
    let owner = h.owner();
    for body in ["one", "two"] {
        assert_eq!(h.publish(&owner, body).await.0, 201);
    }
    let enrollment = h.enroll(&owner, "dev").await;
    let grant = h.grant(&enrollment, json!(["read"])).await;
    assert_eq!(grant["history_after_seq"], 2);
    assert_eq!(grant["principal"]["id"], format!("enrollment:{}", enrollment["enrollment_id"].as_str().unwrap()));
    let (status, issued) = h.issuance(&owner, &grant, 1).await;
    assert_eq!(status, 200, "{issued}");
    let token = grant_token(K1_PEM, "fixture-k1", &issued["grant"]);
    assert_eq!(h.publish(&owner, "three").await.0, 201);

    let (status, page) = h.history(&token, 0).await;
    assert_eq!(status, 200, "{page}");
    assert_eq!(page["skipped"], json!({"after_seq":0,"through_seq":2,"reason":"before_grant_history"}));
    assert_eq!(page["messages"].as_array().unwrap().iter().map(|m| m["seq"].as_u64().unwrap()).collect::<Vec<_>>(), vec![3]);
    assert_eq!(page["next_after_seq"], 3);
    // Legacy array endpoint: explicit refusal below the floor, data at/after it.
    let uri = |after: u64| format!("/v1/threads/{}/messages?after_seq={after}", h.thread);
    assert_eq!(h.call("GET", &uri(0), &token, None).await, (409, json!({"error":"history_cursor_before_floor"})));
    assert_eq!(h.call("GET", &uri(2), &token, None).await.1.as_array().unwrap().len(), 1);
    assert_eq!(h.call("GET", &format!("/v1/threads/{}", h.thread), &token, None).await.0, 200);

    // A read grant cannot publish, whether the token omits or claims publish.
    assert_eq!(h.publish(&token, "nope").await.0, 401);
    let overclaim = sign(K1_PEM, "fixture-k1", &grant_claims(&issued["grant"], json!(["read","publish"])));
    assert_eq!(h.publish(&overclaim, "nope").await, (403, json!({"error":"grant_operation_denied"})));
    // Other threads, global and administrative routes refuse grant credentials.
    let other = format!("/v1/threads/{}/history", uuid::Uuid::new_v4());
    assert_eq!(h.call("GET", &other, &token, None).await.0, 401);
    for (method, uri) in [("GET", "/v1/threads"), ("GET", "/v1/grants"), ("POST", "/v1/enrollments")] {
        assert_eq!(h.call(method, uri, &token, Some(json!({"device_id":"d","session_id":"s"}))).await.0, 401, "{uri}");
    }
    // HS256 is not accepted for grant credentials, even with the legacy secret configured.
    let mut legacy = grant_claims(&issued["grant"], json!(["read"]));
    legacy["iss"] = json!(LEGACY_ISSUER);
    let hs = encode(&Header::default(), &legacy, &EncodingKey::from_secret(LEGACY.as_bytes())).unwrap();
    assert_eq!(h.history(&hs, 2).await.0, 401);

    // Read+publish grant publishes as the enrollment principal.
    let writer = h.enroll(&owner, "writer").await;
    let rw = h.grant(&writer, json!(["publish","read"])).await;
    assert_eq!(rw["operations"], json!(["read","publish"]));
    let rw_token = grant_token(K2_PEM, "fixture-k2", &rw);
    let (status, message) = h.publish(&rw_token, "from device").await;
    assert_eq!(status, 201, "{message}");
    assert_eq!(message["sender"], rw["principal"]);
}

#[tokio::test]
async fn cross_account_and_cross_org_grants_refuse() {
    let h = Harness::new().await;
    let owner = h.owner();
    let enrollment = h.enroll(&owner, "dev").await;
    let grant = h.grant(&enrollment, json!(["read"])).await;
    let grant_uri = format!("/v1/grants/{}", grant["grant_id"].as_str().unwrap());
    let member = owner_token(K1_PEM, "fixture-k1", "org", "member");
    let foreign = owner_token(K2_PEM, "fixture-k2", "org-2", "owner");
    for token in [&member, &foreign] {
        assert_eq!(h.call("GET", &grant_uri, token, None).await.0, 404);
        assert_eq!(h.issuance(token, &grant, 1).await.0, 404);
        assert_eq!(h.call("GET", &format!("/v1/enrollments/{}", enrollment["enrollment_id"].as_str().unwrap()), token, None).await.0, 404);
        assert_eq!(h.call("POST", &format!("{grant_uri}/revoke"), token, Some(json!({}))).await.0, 404);
        // Using someone else's enrollment on a thread.
        let (status, _) = h.call("POST", "/v1/grants", token, Some(json!({"thread_id":h.thread,
            "enrollment_id":enrollment["enrollment_id"],"operations":["read"],"ttl_seconds":3600}))).await;
        assert_eq!(status, 404);
    }
    // A member with its own enrollment still needs invite.
    let theirs = h.enroll(&member, "dev").await;
    let (status, body) = h.call("POST", "/v1/grants", &member, Some(json!({"thread_id":h.thread,
        "enrollment_id":theirs["enrollment_id"],"operations":["read"],"ttl_seconds":3600}))).await;
    assert_eq!((status, body), (403, json!({"error":"invite_required"})));
    // Non-human principals cannot own enrollments.
    let actor = sign(K1_PEM, "fixture-k1", &json!({"iss":SIGNED_ISSUER,"aud":AUDIENCE,"exp":Utc::now().timestamp()+60,
        "jti":"a","principal":{"kind":"actor","id":"bot","org_id":"org"}}));
    assert_eq!(h.call("POST", "/v1/enrollments", &actor, Some(json!({"device_id":"d","session_id":"s"}))).await,
        (403, json!({"error":"enrollment_owner_must_be_human"})));
}

#[tokio::test]
async fn signing_key_rotation_overlap_and_removal() {
    let h = Harness::new().await;
    let enrollment = h.enroll(&h.owner(), "dev").await;
    let grant = h.grant(&enrollment, json!(["read"])).await;
    let old_kid = grant_token(K1_PEM, "fixture-k1", &grant);
    let new_kid = grant_token(K2_PEM, "fixture-k2", &grant);
    let after = grant["history_after_seq"].as_u64().unwrap();
    // Overlap: both kids verify.
    assert_eq!(h.history(&old_kid, after).await.0, 200);
    assert_eq!(h.history(&new_kid, after).await.0, 200);
    // A key signed by k2 but labelled k1 is refused.
    assert_eq!(h.history(&grant_token(K2_PEM, "fixture-k1", &grant), after).await.0, 401);
    // Removal: old kid refuses, new kid keeps working, owner tokens follow too.
    h.verifier.replace_keys(K2_JWKS).unwrap();
    assert_eq!(h.history(&old_kid, after).await.0, 401);
    assert_eq!(h.history(&new_kid, after).await.0, 200);
    assert_eq!(h.call("GET", "/v1/enrollments", &h.owner(), None).await.0, 401);
    assert_eq!(h.call("GET", "/v1/enrollments", &owner_token(K2_PEM, "fixture-k2", "org", "owner"), None).await.0, 200);
}

async fn open_stream(h: &Harness, token: &str) -> Body {
    let response = mq_server::router(h.state.clone()).oneshot(Request::builder()
        .uri(format!("/v1/threads/{}/events", h.thread)).header("authorization", format!("Bearer {token}"))
        .body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    response.into_body()
}

async fn next_revoked(body: &mut Body) {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(7), body.frame()).await
            .expect("stream recheck").expect("frame").unwrap();
        if let Some(data) = frame.data_ref() {
            if std::str::from_utf8(data).unwrap().contains("revoked") {
                return;
            }
        }
    }
}

#[tokio::test]
async fn revoke_restore_and_offline_renew_refusal() {
    let h = Harness::new().await;
    let owner = h.owner();
    let enrollment = h.enroll(&owner, "dev").await;
    let grant = h.grant(&enrollment, json!(["read"])).await;
    let token = grant_token(K1_PEM, "fixture-k1", &grant);
    let mut stream = open_stream(&h, &token).await;
    let (status, revoked) = h.admin(&grant, "revoke", None).await;
    assert_eq!((status, revoked["status"].as_str(), revoked["generation"].as_u64()), (200, Some("revoked"), Some(1)));
    next_revoked(&mut stream).await;
    assert_eq!(h.history(&token, 0).await, (403, json!({"error":"grant_revoked"})));
    // A device that was offline during revoke cannot renew or reissue.
    assert_eq!(h.admin(&grant, "renew", Some(json!({"ttl_seconds":3600}))).await, (403, json!({"error":"grant_revoked"})));
    assert_eq!(h.issuance(&owner, &grant, 1).await, (403, json!({"error":"grant_revoked"})));
    // Restore: old credential stays dead; newly issued one works.
    let (status, restored) = h.admin(&grant, "restore", None).await;
    assert_eq!((status, restored["state"].as_str()), (200, Some("active")));
    assert_eq!(h.history(&token, 0).await, (403, json!({"error":"grant_generation_stale"})));
    let (status, issued) = h.issuance(&owner, &grant, 1).await;
    assert_eq!(status, 200);
    assert_eq!(issued["grant"]["generation"], 1);
    assert_eq!(h.history(&grant_token(K1_PEM, "fixture-k1", &issued["grant"]), 0).await.0, 200);
}

#[tokio::test]
async fn incarnation_fencing_and_expiry() {
    let h = Harness::new().await;
    let owner = h.owner();
    let enrollment = h.enroll(&owner, "dev").await;
    let grant = h.grant(&enrollment, json!(["read"])).await;
    let old_process = grant_token(K1_PEM, "fixture-k1", &grant);
    assert_eq!(h.history(&old_process, 0).await.0, 200);
    let next = h.enroll(&owner, "dev").await;
    assert_eq!(next["incarnation"], 2);
    assert_eq!(h.history(&old_process, 0).await, (403, json!({"error":"grant_incarnation_fenced"})));
    assert_eq!(h.issuance(&owner, &grant, 1).await, (403, json!({"error":"grant_incarnation_fenced"})));
    let (status, issued) = h.issuance(&owner, &grant, 2).await;
    assert_eq!(status, 200);
    let current = grant_token(K1_PEM, "fixture-k1", &issued["grant"]);
    assert_eq!(h.history(&current, 0).await.0, 200);
    // Grant expiry (server clock) refuses a credential that is still valid by exp.
    *h.clock.lock().unwrap() += chrono::Duration::seconds(3601);
    assert_eq!(h.history(&current, 0).await, (403, json!({"error":"grant_expired"})));
    assert_eq!(h.issuance(&owner, &grant, 2).await, (403, json!({"error":"grant_expired"})));
    let (status, renewed) = h.admin(&grant, "renew", Some(json!({"ttl_seconds":600}))).await;
    assert_eq!((status, renewed["state"].as_str()), (200, Some("active")));
    assert_eq!(h.history(&current, 0).await.0, 200);
    // Renew bounds.
    assert_eq!(h.admin(&grant, "renew", Some(json!({"ttl_seconds":59}))).await, (400, json!({"error":"invalid_ttl"})));
}

#[tokio::test]
async fn enrollment_revocation_signs_out_over_http() {
    let h = Harness::new().await;
    let owner = h.owner();
    let enrollment = h.enroll(&owner, "dev").await;
    let grant = h.grant(&enrollment, json!(["read"])).await;
    let token = grant_token(K1_PEM, "fixture-k1", &grant);
    let mut stream = open_stream(&h, &token).await;
    let uri = format!("/v1/enrollments/{}/revoke", enrollment["enrollment_id"].as_str().unwrap());
    let member = owner_token(K1_PEM, "fixture-k1", "org", "member");
    assert_eq!(h.call("POST", &uri, &member, Some(json!({}))).await.0, 404);
    assert_eq!(h.call("POST", &uri, &token, Some(json!({}))).await.0, 401, "grant credentials cannot administer");
    let (status, revoked) = h.call("POST", &uri, &owner, Some(json!({}))).await;
    assert_eq!(status, 200, "{revoked}");
    assert!(revoked["revoked_at"].is_string());
    next_revoked(&mut stream).await;
    assert_eq!(h.history(&token, 0).await, (403, json!({"error":"enrollment_revoked"})));
    assert_eq!(h.issuance(&owner, &grant, 1).await, (403, json!({"error":"enrollment_revoked"})));
    let (status, body) = h.call("POST", "/v1/enrollments", &owner, Some(json!({"device_id":"dev","session_id":"s1"}))).await;
    assert_eq!((status, body), (403, json!({"error":"enrollment_revoked"})));
    let (status, _) = h.call("POST", "/v1/enrollments", &owner, Some(json!({"device_id":"dev","session_id":"s2"}))).await;
    assert_eq!(status, 201, "a new session enrolls independently");
}

#[tokio::test]
async fn delivery_check_is_backend_only_and_reflects_live_grant_state() {
    let h = Harness::new().await;
    let owner = h.owner();
    let enrollment = h.enroll(&owner, "dev").await;
    let grant = h.grant(&enrollment, json!(["read"])).await;
    assert_eq!(h.publish(&owner, "for device").await.0, 201);
    let uri = format!("/v1/grants/{}/delivery-check", grant["grant_id"].as_str().unwrap());
    let body = |generation: u64, incarnation: u64, seq: u64| json!({"generation":generation,"incarnation":incarnation,
        "recipient":grant["principal"],"message_seq":seq});
    let verifier = |org: &str, id: &str, kind: &str| sign(K1_PEM, "fixture-k1", &json!({"iss":SIGNED_ISSUER,"aud":AUDIENCE,
        "exp":Utc::now().timestamp()+60,"jti":uuid::Uuid::new_v4(),"principal":{"kind":kind,"id":id,"org_id":org}}));
    let bridge = verifier("org", "mq-delivery-bridge", "system");
    let (status, verified) = h.call("POST", &uri, &bridge, Some(body(0, 1, 1))).await;
    assert_eq!(status, 200, "{verified}");
    assert_eq!(verified, json!({"grant_id":grant["grant_id"],"generation":0,"incarnation":1}));
    // Only the backend-signed verifier of the grant's org may ask.
    assert_eq!(h.call("POST", &uri, &owner, Some(body(0, 1, 1))).await, (403, json!({"error":"delivery_verifier_required"})));
    assert_eq!(h.call("POST", &uri, &verifier("org-2", "mq-delivery-bridge", "system"), Some(body(0, 1, 1))).await.0, 404);
    let mut legacy = json!({"iss":LEGACY_ISSUER,"aud":AUDIENCE,"exp":Utc::now().timestamp()+60,"jti":"x",
        "principal":{"kind":"system","id":"mq-delivery-bridge","org_id":"org"}});
    let hs = encode(&Header::default(), &legacy, &EncodingKey::from_secret(LEGACY.as_bytes())).unwrap();
    assert_eq!(h.call("POST", &uri, &hs, Some(body(0, 1, 1))).await.0, 401, "HS256 cannot verify deliveries");
    legacy["iss"] = json!(SIGNED_ISSUER);
    assert_eq!(h.call("POST", &uri, &grant_token(K1_PEM, "fixture-k1", &grant), Some(body(0, 1, 1))).await.0, 401);
    // Stale generation/incarnation, below-floor messages and revocation refuse.
    assert_eq!(h.call("POST", &uri, &bridge, Some(body(1, 1, 1))).await, (403, json!({"error":"grant_generation_stale"})));
    assert_eq!(h.call("POST", &uri, &bridge, Some(body(0, 2, 1))).await, (403, json!({"error":"grant_incarnation_fenced"})));
    assert_eq!(h.call("POST", &uri, &bridge, Some(body(0, 1, 0))).await, (403, json!({"error":"grant_operation_denied"})));
    let mut other = body(0, 1, 1);
    other["recipient"]["id"] = json!("enrollment:someone-else");
    assert_eq!(h.call("POST", &uri, &bridge, Some(other)).await, (403, json!({"error":"grant_operation_denied"})));
    assert_eq!(h.admin(&grant, "revoke", None).await.0, 200);
    assert_eq!(h.call("POST", &uri, &bridge, Some(body(0, 1, 1))).await, (403, json!({"error":"grant_revoked"})));
    assert_eq!(h.call("POST", &uri, &bridge, Some(body(1, 1, 1))).await, (403, json!({"error":"grant_revoked"})));
}

#[test]
fn reserved_principal_helpers_are_consistent() {
    let id = uuid::Uuid::new_v4();
    let p: Principal = mq_core::grants::enrollment_principal("org", id);
    assert_eq!(p.kind, PrincipalKind::Actor);
    assert!(mq_core::grants::is_enrollment_principal(&p));
    let _unused: Option<Grant> = None;
}
