//! Store-level qualification of the MQ mailbox persistence: registered
//! schema restart/isolation, native acceptance fences, authorized history
//! gaps, account/grant write fences and history reconciliation.
use super::*;
use crate::cloud::mailbox::policy::{ParticipantPolicy, Preset};
use crate::storage::Storage;
use mq_core::{Message, MessageId, MessageKind, Principal, PrincipalKind, ThreadId};
use tempfile::{tempdir, TempDir};

fn identity(account: &str) -> CloudScopeIdentity {
    CloudScopeIdentity {
        backend_origin: "https://fixture.invalid".into(),
        backend_id: "backend-fixture".into(),
        account_id: account.into(),
        org_id: "org".into(),
        profile_id: "fixture".into(),
    }
}

fn enrollment(incarnation: u64) -> EnrollmentBinding {
    EnrollmentBinding {
        enrollment_id: "e1".into(),
        device_id: "d1".into(),
        incarnation,
        principal_id: "enrollment:e1".into(),
        org_id: "org".into(),
        mq_endpoint: "https://mq.example.test".into(),
    }
}

fn grant(thread: &str, generation: u64, incarnation: u64, lifecycle: GrantLifecycle) -> GrantSnapshot {
    GrantSnapshot {
        grant_id: "g1".into(),
        thread_id: thread.into(),
        enrollment_id: "e1".into(),
        principal_id: "enrollment:e1".into(),
        org_id: "org".into(),
        operations: vec!["publish".into(), "read".into()],
        history_after_seq: 2,
        expires_at_ms: chrono::Utc::now().timestamp_millis() + 3_600_000,
        incarnation,
        generation,
        lifecycle,
    }
}

struct Fixture {
    _dir: TempDir,
    db: Arc<Database>,
    store: CloudStore,
    lease: ScopeLease,
    thread: String,
}

fn fixture() -> Fixture {
    let dir = tempdir().unwrap();
    let db = Storage::open(dir.path()).unwrap().database().clone();
    db.with_conn(|conn| {
        conn.execute("INSERT INTO sessions(id,title,kind,target_json,runtime_target_kind,status,metadata_json,created_at,updated_at) VALUES('local-1','Local','codex','{}','local','ready','{}','t','t')", [])?;
        conn.execute("INSERT INTO sessions(id,title,kind,target_json,runtime_target_kind,status,metadata_json,created_at,updated_at) VALUES('local-2','Local 2','codex','{}','local','ready','{}','t','t')", [])?;
        Ok(())
    })
    .unwrap();
    let store = CloudStore::open(db.clone()).unwrap();
    let lease = store.activate_verified(&identity("a")).unwrap();
    let thread = uuid::Uuid::from_u128(42).to_string();
    let spec = spec(&thread, "local-1");
    store.connect_mq_participant(&lease, &spec, &enrollment(1)).unwrap();
    store.attach_mq_grant(&lease, &thread, &grant(&thread, 0, 1, GrantLifecycle::Active)).unwrap();
    Fixture { _dir: dir, db, store, lease, thread }
}

fn spec(thread: &str, session: &str) -> ParticipantSpec {
    ParticipantSpec {
        thread_id: thread.into(),
        local_session_id: session.into(),
        peers: vec![PeerRef { kind: "actor".into(), id: "peer".into(), org_id: "org".into() }],
        preset: Preset::Respond,
        policy: ParticipantPolicy::default(),
    }
}

fn message(thread: &str, seq: u64, sender: &str, org: &str) -> Message {
    Message {
        message_id: MessageId::new(),
        thread_id: ThreadId(uuid::Uuid::parse_str(thread).unwrap()),
        seq,
        kind: MessageKind::Ask,
        body: format!("message {seq}"),
        payload: json!({}),
        sender: Principal { kind: PrincipalKind::Actor, id: sender.into(), org_id: org.into() },
        idempotency_key: None,
        correlation_id: Some(format!("corr-{seq}")),
        parent_message_id: None,
        causation_id: None,
        created_at: chrono::Utc::now(),
    }
}

fn page(thread: &str, cursor: u64, messages: Vec<Message>) -> MqHistoryPage {
    let effective = cursor.max(2);
    MqHistoryPage {
        thread_id: ThreadId(uuid::Uuid::parse_str(thread).unwrap()),
        requested_after_seq: cursor,
        history_after_seq: 2,
        effective_after_seq: effective,
        skipped: (cursor < 2).then(|| MqSkipped { after_seq: cursor, through_seq: 2, reason: "before_grant_history".into() }),
        next_after_seq: messages.last().map_or(effective, |m| m.seq),
        messages,
        has_more: false,
    }
}

fn draft(local: &str) -> OutboundDraft {
    OutboundDraft {
        local_message_id: local.into(),
        kind: MessageKind::Ask,
        body: "question".into(),
        payload: json!({}),
        correlation_id: Some("corr".into()),
        causation_id: Some("cause".into()),
        parent_message_id: None,
        recipients: vec![PeerRef { kind: "actor".into(), id: "peer".into(), org_id: "org".into() }],
        disposition: OutboundDisposition::Message,
        reply_to_message_id: None,
        causal_depth: 0,
    }
}

fn fence(generation: u64, incarnation: u64, session: &str) -> DeliveryFence {
    DeliveryFence { local_session_id: session.into(), incarnation, grant_generation: generation }
}

#[test]
fn registered_store_survives_restart_and_keeps_accounts_isolated() {
    let dir = tempdir().unwrap();
    let thread = uuid::Uuid::from_u128(7).to_string();
    {
        let db = Storage::open(dir.path()).unwrap().database().clone();
        db.with_conn(|conn| { conn.execute("INSERT INTO sessions(id,title,kind,target_json,runtime_target_kind,status,metadata_json,created_at,updated_at) VALUES('local-1','Local','codex','{}','local','ready','{}','t','t')", [])?; Ok(()) }).unwrap();
        let store = CloudStore::open(db).unwrap();
        let lease = store.activate_verified(&identity("a")).unwrap();
        store.connect_mq_participant(&lease, &spec(&thread, "local-1"), &enrollment(1)).unwrap();
        store.attach_mq_grant(&lease, &thread, &grant(&thread, 0, 1, GrantLifecycle::Active)).unwrap();
        store.enqueue_mq_publish(&lease, &thread, &draft("restart-1")).unwrap();
    }
    // Restart: reopening applies no migration twice and keeps every row.
    let storage = Storage::open(dir.path()).unwrap();
    let db = storage.database().clone();
    let version: i64 = db.with_conn(|conn| Ok(conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))?)).unwrap();
    assert!(version >= 69);
    let store = CloudStore::open(db.clone()).unwrap();
    let a = store.activate_verified(&identity("a")).unwrap();
    assert_eq!(store.mq_participant(&a, &thread).unwrap().unwrap().local_session_id, "local-1");
    assert_eq!(store.mq_outbox(&a, &thread, 10).unwrap()[0].status, "queued", "expiry/restart alone does not fence same-account writes");
    let b = store.activate_verified(&identity("b")).unwrap();
    assert!(store.mq_participant(&b, &thread).unwrap().is_none());
    assert!(store.mq_outbox(&b, &thread, 10).unwrap().is_empty());
    assert!(store.connect_mq_participant(&b, &spec(&thread, "local-1"), &enrollment(1)).is_err(), "another account cannot adopt the session");
    assert!(store.mq_participant(&a, &thread).is_err(), "stale lease after account switch");
    // A same-named table with another shape refuses the store; local stays usable.
    db.with_conn(|conn| { conn.execute_batch("DROP TABLE cloud_mq_history_gaps; CREATE TABLE cloud_mq_history_gaps(unrelated TEXT);")?; Ok(()) }).unwrap();
    assert!(CloudStore::open(db.clone()).is_err());
    assert!(Storage::open(dir.path()).is_ok());
}

#[test]
fn history_records_the_authorized_gap_and_refuses_real_gaps() {
    let f = fixture();
    let first = page(&f.thread, 0, vec![message(&f.thread, 3, "peer", "org"), message(&f.thread, 4, "peer", "org")]);
    let commit = f.store.commit_mq_history(&f.lease, &f.thread, None, &first).unwrap();
    assert_eq!((commit.committed, commit.gap, commit.cursor), (2, Some((0, 2)), 4));
    assert_eq!(f.store.mq_history_gaps(&f.lease, &f.thread).unwrap(), vec![(0, 2, "before_grant_history".into())]);
    let (checkpoint, cursor) = f.store.mq_history_cursor(&f.lease, &f.thread).unwrap();
    assert_eq!(cursor, 4);
    // A skip claimed after the floor, a real gap, a foreign sender and a
    // changed floor all refuse without moving the cursor.
    let mut bogus_skip = page(&f.thread, 4, vec![message(&f.thread, 5, "peer", "org")]);
    bogus_skip.skipped = Some(MqSkipped { after_seq: 4, through_seq: 5, reason: "x".into() });
    let gap = page(&f.thread, 4, vec![message(&f.thread, 6, "peer", "org")]);
    let foreign = page(&f.thread, 4, vec![message(&f.thread, 5, "peer", "other-org")]);
    let mut floor = page(&f.thread, 4, vec![message(&f.thread, 5, "peer", "org")]);
    floor.history_after_seq = 1;
    for bad in [bogus_skip, gap, foreign, floor] {
        assert!(f.store.commit_mq_history(&f.lease, &f.thread, checkpoint.as_ref(), &bad).is_err());
    }
    assert_eq!(f.store.mq_history_cursor(&f.lease, &f.thread).unwrap().1, 4);
    assert_eq!(f.store.mq_deliveries(&f.lease, &f.thread, &["delivered"], 10).unwrap().len(), 2);
    // Identical replay is idempotent.
    let replay = f.store.commit_mq_history(&f.lease, &f.thread, checkpoint.as_ref(), &page(&f.thread, 4, vec![])).unwrap();
    assert_eq!(replay.committed, 0);
}

#[test]
fn native_acceptance_fences_on_session_incarnation_and_grant_generation() {
    let f = fixture();
    let first = message(&f.thread, 3, "peer", "org");
    let id = first.message_id.0.to_string();
    f.store.commit_mq_history(&f.lease, &f.thread, None, &page(&f.thread, 0, vec![first])).unwrap();
    for wrong in [fence(0, 2, "local-1"), fence(1, 1, "local-1"), fence(0, 1, "local-2")] {
        assert!(f.store.observe_mq_delivery(&f.lease, &f.thread, &id, &wrong).is_err());
    }
    assert_eq!(f.store.mq_delivery(&f.lease, &f.thread, &id).unwrap().stage, "delivered");
    let DeliveryAdmission::Observed(view) = f.store.observe_mq_delivery(&f.lease, &f.thread, &id, &fence(0, 1, "local-1")).unwrap() else { panic!("expected observation") };
    assert_eq!(view.stage, "observed");
    f.db.with_conn(|conn| {
        let handoff: i64 = conn.query_row("SELECT COUNT(*) FROM command_receipts WHERE kind='mq.input'", [], |r| r.get(0))?;
        let journal: i64 = conn.query_row("SELECT COUNT(*) FROM events WHERE kind='mq.delivery.observed' AND session_id='local-1'", [], |r| r.get(0))?;
        assert_eq!((handoff, journal), (1, 1));
        Ok(())
    }).unwrap();
    // Idempotent re-observation.
    assert!(matches!(f.store.observe_mq_delivery(&f.lease, &f.thread, &id, &fence(0, 1, "local-1")).unwrap(), DeliveryAdmission::Observed(_)));

    // Delivered under generation 0, then revoke+restore (generation 1).
    let (checkpoint, _) = f.store.mq_history_cursor(&f.lease, &f.thread).unwrap();
    let second = message(&f.thread, 4, "peer", "org");
    let second_id = second.message_id.0.to_string();
    f.store.commit_mq_history(&f.lease, &f.thread, checkpoint.as_ref(), &page(&f.thread, 3, vec![second])).unwrap();
    f.store.attach_mq_grant(&f.lease, &f.thread, &grant(&f.thread, 1, 1, GrantLifecycle::Active)).unwrap();
    assert_eq!(f.store.mq_delivery(&f.lease, &f.thread, &second_id).unwrap().stage, "fenced");
    assert!(f.store.observe_mq_delivery(&f.lease, &f.thread, &second_id, &fence(1, 1, "local-1")).is_err());
    assert!(f.store.attach_mq_grant(&f.lease, &f.thread, &grant(&f.thread, 0, 1, GrantLifecycle::Active)).is_err(), "generation cannot regress");

    // A newer process incarnation supersedes this one.
    f.store.refresh_mq_incarnation(&f.lease, &f.thread, &enrollment(2)).unwrap();
    assert!(f.store.refresh_mq_incarnation(&f.lease, &f.thread, &enrollment(1)).is_err());
    assert!(f.store.begin_mq_acting(&f.lease, &f.thread, &id, &fence(1, 1, "local-1"), chrono::Utc::now().timestamp_millis()).is_err());
    assert!(f.store.attach_mq_grant(&f.lease, &f.thread, &grant(&f.thread, 1, 1, GrantLifecycle::Active)).is_err(), "grant must carry the current incarnation");
}

#[test]
fn queued_writes_fence_on_account_switch_signout_and_generation_but_survive_expiry() {
    let f = fixture();
    let first = f.store.enqueue_mq_publish(&f.lease, &f.thread, &draft("w-1")).unwrap();
    assert_eq!(first, f.store.enqueue_mq_publish(&f.lease, &f.thread, &draft("w-1")).unwrap());
    let mut changed = draft("w-1");
    changed.body = "different".into();
    assert!(f.store.enqueue_mq_publish(&f.lease, &f.thread, &changed).is_err());
    let mut stranger = draft("w-x");
    stranger.recipients = vec![PeerRef { kind: "actor".into(), id: "not-a-peer".into(), org_id: "org".into() }];
    assert!(f.store.enqueue_mq_publish(&f.lease, &f.thread, &stranger).is_err(), "recipients must be named participants");

    // Expiry (plain epoch invalidation) keeps the same-account queue sendable.
    f.store.sign_out().unwrap();
    let lease = f.store.activate_verified(&identity("a")).unwrap();
    let SendAdmission::Send(request) = f.store.begin_mq_send(&lease, &f.thread, "mq-publish:w-1").unwrap() else { panic!("expected send") };
    assert_eq!(request.publish.idempotency_key.as_deref(), Some("workshop:w-1"));
    assert_eq!(request.publish.correlation_id.as_deref(), Some("corr"));
    assert_eq!(request.publish.causation_id.as_deref(), Some("cause"));
    assert!(f.store.begin_mq_send(&lease, &f.thread, "mq-publish:w-1").is_err(), "one claimant only");

    // Account switch and explicit sign-out fence permanently.
    f.store.enqueue_mq_publish(&lease, &f.thread, &draft("w-2")).unwrap();
    f.store.activate_verified(&identity("b")).unwrap();
    let lease = f.store.activate_verified(&identity("a")).unwrap();
    assert_eq!(f.store.mq_outbox_entry(&lease, "mq-publish:w-2").unwrap().status, "fenced");
    assert!(matches!(f.store.begin_mq_send(&lease, &f.thread, "mq-publish:w-2").unwrap(), SendAdmission::Fenced(reason) if reason == "account_signed_out"));
    f.store.enqueue_mq_publish(&lease, &f.thread, &draft("w-3")).unwrap();
    f.store.sign_out_explicit().unwrap();
    let lease = f.store.activate_verified(&identity("a")).unwrap();
    assert!(matches!(f.store.begin_mq_send(&lease, &f.thread, "mq-publish:w-3").unwrap(), SendAdmission::Fenced(_)));

    // A new grant generation fences writes captured under the old one.
    f.store.enqueue_mq_publish(&lease, &f.thread, &draft("w-4")).unwrap();
    f.store.attach_mq_grant(&lease, &f.thread, &grant(&f.thread, 1, 1, GrantLifecycle::Active)).unwrap();
    let fenced = f.store.mq_outbox_entry(&lease, "mq-publish:w-4").unwrap();
    assert_eq!((fenced.status.as_str(), fenced.fenced_reason.as_deref()), ("fenced", Some("grant_generation_fenced")));
    // Revocation fences everything and refuses new writes.
    f.store.enqueue_mq_publish(&lease, &f.thread, &draft("w-5")).unwrap();
    f.store.attach_mq_grant(&lease, &f.thread, &grant(&f.thread, 2, 1, GrantLifecycle::Revoked)).unwrap();
    assert_eq!(f.store.mq_outbox_entry(&lease, "mq-publish:w-5").unwrap().fenced_reason.as_deref(), Some("grant_revoked"));
    assert!(f.store.enqueue_mq_publish(&lease, &f.thread, &draft("w-6")).is_err());
}

#[test]
fn uncertain_sends_settle_only_through_matching_history() {
    let f = fixture();
    f.store.enqueue_mq_publish(&f.lease, &f.thread, &draft("u-1")).unwrap();
    f.store.enqueue_mq_publish(&f.lease, &f.thread, &draft("u-2")).unwrap();
    for id in ["mq-publish:u-1", "mq-publish:u-2"] {
        assert!(matches!(f.store.begin_mq_send(&f.lease, &f.thread, id).unwrap(), SendAdmission::Send(_)));
    }
    assert!(f.store.record_mq_rejected(&f.lease, "mq-publish:u-1", false, &json!({})).is_ok());
    assert_eq!(f.store.mq_outbox_entry(&f.lease, "mq-publish:u-1").unwrap().status, "refused");
    // Our own publication appears in history with identical semantics.
    let mut own = message(&f.thread, 3, "enrollment:e1", "org");
    own.idempotency_key = Some("workshop:u-2".into());
    own.body = "question".into();
    own.correlation_id = Some("corr".into());
    own.causation_id = Some("cause".into());
    let own_id = own.message_id.0.to_string();
    let commit = f.store.commit_mq_history(&f.lease, &f.thread, None, &page(&f.thread, 0, vec![own])).unwrap();
    assert_eq!(commit.own_reconciled, vec!["mq-publish:u-2".to_owned()]);
    let settled = f.store.mq_outbox_entry(&f.lease, "mq-publish:u-2").unwrap();
    assert_eq!((settled.status.as_str(), settled.mq_message_id.as_deref()), ("accepted", Some(own_id.as_str())));
    assert!(f.store.mq_deliveries(&f.lease, &f.thread, &[], 10).unwrap().is_empty(), "own publications are not inbox input");
    // A same-key publication with different semantics is a conflict, not success.
    f.store.enqueue_mq_publish(&f.lease, &f.thread, &draft("u-3")).unwrap();
    f.store.begin_mq_send(&f.lease, &f.thread, "mq-publish:u-3").unwrap();
    let (checkpoint, _) = f.store.mq_history_cursor(&f.lease, &f.thread).unwrap();
    let mut forged = message(&f.thread, 4, "enrollment:e1", "org");
    forged.idempotency_key = Some("workshop:u-3".into());
    f.store.commit_mq_history(&f.lease, &f.thread, checkpoint.as_ref(), &page(&f.thread, 3, vec![forged])).unwrap();
    assert_eq!(f.store.mq_outbox_entry(&f.lease, "mq-publish:u-3").unwrap().status, "conflict");
}
