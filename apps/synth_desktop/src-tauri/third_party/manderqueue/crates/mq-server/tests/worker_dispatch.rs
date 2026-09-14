//! Loop-level worker test: a queued or leased delivery for a revoked, expired,
//! generation-fenced or signed-out grant is never sent to the bridge.
//! Fake loopback bridge; fixture delivery secret only.

use std::sync::{Arc, Mutex};

use axum::{extract::State, routing::post, Json, Router};
use chrono::{DateTime, Duration, Utc};
use mq_core::*;
use mq_server::delivery::DELIVERY_PATH;
use mq_server::worker::{dispatch_job, http_client, run_once, WorkerConfig};
use serde_json::{json, Value};

const SECRET: &str = "fixture-delivery-secret-at-least-32-bytes";

type Seen = Arc<Mutex<Vec<Value>>>;

async fn bridge(State(seen): State<Seen>, Json(body): Json<Value>) -> Json<Value> {
    seen.lock().unwrap().push(body.clone());
    let mut receipt = json!({"status": "awaiting_pull"});
    for key in ["job_id", "message_id", "thread_id", "recipient", "attempts", "grant"] {
        if let Some(value) = body.get(key) {
            receipt[key] = value.clone();
        }
    }
    Json(receipt)
}

struct World {
    mq: Fabric,
    store: MemoryStore,
    clock: Arc<Mutex<DateTime<Utc>>>,
    seen: Seen,
    config: WorkerConfig,
    http: reqwest::Client,
    owner: Principal,
    thread: ThreadId,
    enrollment: Enrollment,
    grant: Grant,
}

impl World {
    async fn new() -> Self {
        let seen: Seen = Arc::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route(DELIVERY_PATH, post(bridge)).with_state(seen.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let store = MemoryStore::default();
        let clock = Arc::new(Mutex::new(Utc::now()));
        let source = clock.clone();
        let mq = Fabric::from_store(Arc::new(store.clone())).with_clock(Arc::new(move || *source.lock().unwrap()));
        let owner = Principal { kind: PrincipalKind::Human, id: "owner".into(), org_id: "org".into() };
        let thread = mq.create_thread(&owner, CreateThread {
            org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
            participants: vec![Participant::new(owner.clone(), Role::Owner)], idempotency_key: None,
        }).await.unwrap().thread_id;
        let enrollment = mq.enroll(&owner, EnrollDevice { device_id: "dev".into(), session_id: "s1".into(), label: None }).await.unwrap();
        let grant = mq.create_grant(&owner, CreateGrant {
            thread_id: thread, enrollment_id: enrollment.enrollment_id, operations: vec![GrantOperation::Read],
            ttl_seconds: 3600, history_after_seq: None,
        }).await.unwrap();
        let config = WorkerConfig::new(&format!("http://{addr}"), SECRET, 3).unwrap();
        Self { mq, store, clock, seen, config, http: http_client(), owner, thread, enrollment, grant }
    }

    async fn publish(&self, body: &str) -> Message {
        self.mq.publish(&self.owner, self.thread, PublishMessage { body: body.into(), ..Default::default() }).await.unwrap()
    }

    fn requests(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn status_of(&self, message: &Message) -> DeliveryStatus {
        self.store.checkpoint().jobs.into_iter()
            .find(|j| j.message_id == message.message_id && j.recipient == self.grant.principal)
            .expect("job for device").status
    }

    async fn run(&self) -> Vec<mq_server::worker::JobOutcome> {
        run_once(&self.mq, &self.http, &self.config, 10).await.unwrap()
    }
}

#[tokio::test]
async fn live_grant_is_dispatched_with_its_grant_authority() {
    let w = World::new().await;
    let message = w.publish("live").await;
    let outcomes = w.run().await;
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].dispatched);
    assert_eq!(outcomes[0].settle, DeliveryStatus::AwaitingPull);
    let sent = w.seen.lock().unwrap()[0].clone();
    assert_eq!(sent["grant"], json!({"grant_id": w.grant.grant_id, "generation": 0, "incarnation": 1}));
    assert_eq!(sent["recipient"]["id"], w.grant.principal.id.as_str());
    assert_eq!(w.status_of(&message), DeliveryStatus::AwaitingPull);
}

#[tokio::test]
async fn grant_revoked_while_the_job_is_leased_is_never_dispatched() {
    let w = World::new().await;
    let message = w.publish("leased").await;
    let leased = w.mq.claim_delivery_jobs(10).await.unwrap();
    assert_eq!(leased.len(), 1);
    w.mq.revoke_grant(&w.owner, w.grant.grant_id).await.unwrap();
    let outcome = dispatch_job(&w.mq, &w.http, &w.config, leased[0].clone()).await;
    assert!(!outcome.dispatched);
    assert_eq!(outcome.reason, "grant_denied");
    assert_eq!(w.requests(), 0);
    assert_eq!(w.status_of(&message), DeliveryStatus::DeadLetter);
}

#[tokio::test]
async fn expired_grant_queued_delivery_is_dead_lettered_without_a_request() {
    let w = World::new().await;
    let message = w.publish("queued").await;
    *w.clock.lock().unwrap() += Duration::seconds(3601);
    let outcomes = w.run().await;
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].dispatched);
    assert_eq!(w.requests(), 0);
    assert_eq!(w.status_of(&message), DeliveryStatus::DeadLetter);
}

#[tokio::test]
async fn generation_fenced_delivery_stays_dead_after_restore() {
    let w = World::new().await;
    let before = w.publish("before revoke").await;
    w.mq.revoke_grant(&w.owner, w.grant.grant_id).await.unwrap();
    w.mq.restore_grant(&w.owner, w.grant.grant_id).await.unwrap();
    // The pre-revoke job was dead-lettered by the revoke and is not resurrected.
    assert!(w.run().await.is_empty());
    assert_eq!(w.requests(), 0);
    assert_eq!(w.status_of(&before), DeliveryStatus::DeadLetter);
    // New traffic after restore carries the advanced generation.
    let after = w.publish("after restore").await;
    let outcomes = w.run().await;
    assert!(outcomes.iter().all(|o| o.dispatched));
    assert_eq!(w.seen.lock().unwrap()[0]["grant"]["generation"], 1);
    assert_eq!(w.status_of(&after), DeliveryStatus::AwaitingPull);
}

#[tokio::test]
async fn signed_out_or_membership_revoked_device_is_never_dispatched() {
    let w = World::new().await;
    let message = w.publish("sign-out race").await;
    let leased = w.mq.claim_delivery_jobs(10).await.unwrap();
    w.mq.revoke_enrollment(&w.owner, w.enrollment.enrollment_id).await.unwrap();
    assert!(!dispatch_job(&w.mq, &w.http, &w.config, leased[0].clone()).await.dispatched);
    assert_eq!(w.status_of(&message), DeliveryStatus::DeadLetter);

    let w = World::new().await;
    let message = w.publish("membership race").await;
    let leased = w.mq.claim_delivery_jobs(10).await.unwrap();
    // Membership revocation dead-letters queued work; a leased job is still rechecked.
    w.mq.set_participant_role(&w.owner, w.thread, &w.grant.principal, Role::Revoked).await.unwrap();
    assert!(!dispatch_job(&w.mq, &w.http, &w.config, leased[0].clone()).await.dispatched);
    assert_eq!(w.requests(), 0);
    assert_eq!(w.status_of(&message), DeliveryStatus::DeadLetter);
}

#[tokio::test]
async fn incarnation_change_redirects_queued_delivery_to_the_current_process() {
    let w = World::new().await;
    w.publish("queued before restart").await;
    let leased = w.mq.claim_delivery_jobs(10).await.unwrap();
    assert_eq!(w.mq.enroll(&w.owner, EnrollDevice { device_id: "dev".into(), session_id: "s1".into(), label: None }).await.unwrap().incarnation, 2);
    // Never dispatched under the fenced incarnation: the envelope names the
    // current one, so the old process cannot accept it (bridge/native fence).
    let outcome = dispatch_job(&w.mq, &w.http, &w.config, leased[0].clone()).await;
    assert!(outcome.dispatched);
    assert_eq!(w.seen.lock().unwrap()[0]["grant"]["incarnation"], 2);
}

#[tokio::test]
async fn ordinary_recipients_keep_the_existing_dispatch_path() {
    let w = World::new().await;
    let member = Principal { kind: PrincipalKind::Human, id: "member".into(), org_id: "org".into() };
    w.mq.add_participant(&w.owner, w.thread, Participant::new(member.clone(), Role::Member)).await.unwrap();
    w.publish("to everyone").await;
    let outcomes = w.run().await;
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|o| o.dispatched));
    let seen = w.seen.lock().unwrap();
    let to_member = seen.iter().find(|b| b["recipient"]["id"] == "member").unwrap();
    assert!(to_member.get("grant").is_none());
    assert!(WorkerConfig::new("http://user:pw@bridge.test", SECRET, 3).is_err());
    assert!(WorkerConfig::new("http://bridge.test/path", SECRET, 3).is_err());
    assert!(WorkerConfig::new("http://bridge.test", "short", 3).is_err());
}
