use super::*;
use crate::domain::{
    RunCreate, RunService, RuntimeTarget, SessionCreate, SessionKind, SessionService, SessionStatus,
};
use crate::storage::{EventSource, Storage};
use std::sync::Arc;

const PREVIOUS_BOOT: &str = "inst_previous";
const CURRENT_BOOT: &str = "inst_current";

struct Fixture {
    _dir: tempfile::TempDir,
    storage: Storage,
    sessions: SessionService,
    runs: RunService,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        Self {
            _dir: dir,
            storage,
            sessions: SessionService::new(db.clone()),
            runs: RunService::new(db),
        }
    }

    fn db(&self) -> &Arc<crate::storage::Database> {
        self.storage.database()
    }

    async fn session(&self, id: &str) -> crate::storage::SessionRecord {
        self.sessions.get(id.to_owned()).await.unwrap().unwrap()
    }

    async fn run(&self, id: &str) -> crate::storage::RunRecord {
        self.runs.get(id.to_owned()).await.unwrap().unwrap()
    }

    /// A chat mid-turn, exactly as the previous process left it.
    async fn abandoned_turn(&self, session_id: &str, run_id: &str, prompt: &str) {
        self.sessions
            .create_or_update(SessionCreate {
                id: session_id.into(),
                title: "Craftax eval".into(),
                kind: SessionKind::Codex,
                target: RuntimeTarget::from_codex_provider("openrouter", "gpt-5.6-luna"),
                project_id: None,
                remote_id: None,
                codex_thread_id: Some("thread-201".into()),
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: serde_json::json!({"titleOrigin":"default","model":"gpt-5.6-luna"}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        let session_id_owned = session_id.to_owned();
        let prompt = prompt.to_owned();
        self.db()
            .run_transaction(move |conn| {
                crate::storage::append_event(
                    conn,
                    crate::storage::EventAppend::codex(
                        session_id_owned,
                        "message.created",
                        serde_json::json!({
                            "messageId": "user-1",
                            "role": "user",
                            "content": prompt,
                        }),
                    ),
                )?;
                Ok(())
            })
            .await
            .unwrap();
        self.runs
            .start(RunCreate {
                id: run_id.into(),
                session_id: session_id.into(),
                mode: "codex_turn".into(),
                model: Some("gpt-5.6-luna".into()),
                adapter: None,
                metadata: serde_json::json!({"threadId":"thread-201","effort":"xhigh"}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        let session_id_owned = session_id.to_owned();
        let run_id_owned = run_id.to_owned();
        // A previous boot that is actually gone: heartbeat older than the lease.
        // A fresh heartbeat from another owner is a live peer and must not be
        // interrupted — see `a_live_peer_is_not_interrupted`.
        let dead_at = Utc::now() - LEASE_DURATION - Duration::seconds(1);
        self.db()
            .run_transaction(move |conn| {
                ownership::claim(
                    conn,
                    &session_id_owned,
                    &run_id_owned,
                    PREVIOUS_BOOT,
                    Some("attach-1"),
                    0,
                    dead_at,
                )
            })
            .await
            .unwrap();
    }

    async fn reconcile(&self) -> Vec<RecoveryNotice> {
        self.reconcile_at(Utc::now()).await
    }

    async fn reconcile_at(&self, now: DateTime<Utc>) -> Vec<RecoveryNotice> {
        self.db()
            .run_transaction(move |conn| reconcile_orphaned_turns(conn, CURRENT_BOOT, now))
            .await
            .unwrap()
    }

    async fn recovery_events(&self, session_id: &str) -> usize {
        let session_id = session_id.to_owned();
        self.db()
            .run(move |conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM events WHERE session_id = ?1 AND kind = ?2",
                    rusqlite::params![session_id, RECOVERY_EVENT_KIND],
                    |row| row.get::<_, i64>(0),
                )?)
            })
            .await
            .unwrap() as usize
    }
}

#[tokio::test]
async fn a_turn_owned_by_a_dead_process_is_interrupted_at_startup() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn(
            "session-201",
            "turn-201",
            "Run the Craftax eval for seed 201",
        )
        .await;
    assert_eq!(fixture.session("session-201").await.status, "running");

    let notices = fixture.reconcile().await;

    assert_eq!(notices.len(), 1);
    let notice = &notices[0];
    assert_eq!(notice.reason, RecoveryReason::WorkshopRestarted.as_str());
    assert_eq!(
        notice.previous_owner_instance_id.as_deref(),
        Some(PREVIOUS_BOOT)
    );
    assert_eq!(notice.run_id.as_deref(), Some("turn-201"));
    assert_eq!(notice.recovery_attempt, 1);
    assert!(notice.restartable);
    assert!(!notice.needs_attention);

    let session = fixture.session("session-201").await;
    assert_eq!(session.status, "interrupted");
    assert!(session.active_run_id.is_none());
    assert_eq!(fixture.run("turn-201").await.status, "interrupted");
}

#[tokio::test]
async fn reconciliation_journals_once_and_is_idempotent() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-202", "turn-202", "Run seed 202")
        .await;

    assert_eq!(fixture.reconcile().await.len(), 1);
    assert_eq!(fixture.reconcile().await.len(), 0);
    assert_eq!(fixture.reconcile().await.len(), 0);

    assert_eq!(fixture.recovery_events("session-202").await, 1);
    assert_eq!(fixture.session("session-202").await.status, "interrupted");
}

#[tokio::test]
async fn settled_sessions_are_left_alone() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-203", "turn-203", "Run seed 203")
        .await;
    fixture
        .runs
        .transition(
            "turn-203".into(),
            crate::domain::RunStatus::Completed,
            Some(serde_json::json!({"ok": true})),
            EventSource::Codex,
        )
        .await
        .unwrap();
    let before = fixture.session("session-203").await;

    assert!(fixture.reconcile().await.is_empty());

    let after = fixture.session("session-203").await;
    assert_eq!(after.status, "ready");
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(fixture.run("turn-203").await.status, "completed");
    assert_eq!(fixture.recovery_events("session-203").await, 0);
}

#[tokio::test]
async fn a_turn_this_process_still_owns_keeps_running() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-204", "turn-204", "Run seed 204")
        .await;
    // Hand the claim to the current boot epoch, as a live turn would.
    fixture
        .db()
        .run_transaction(|conn| {
            ownership::claim(
                conn,
                "session-204",
                "turn-204",
                CURRENT_BOOT,
                Some("attach-2"),
                0,
                Utc::now(),
            )
        })
        .await
        .unwrap();

    assert!(fixture.reconcile().await.is_empty());
    assert_eq!(fixture.session("session-204").await.status, "running");
    assert_eq!(fixture.run("turn-204").await.status, "running");
}

#[tokio::test]
async fn a_live_peer_is_not_interrupted() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-live-peer", "turn-live-peer", "Run seed live")
        .await;
    fixture
        .db()
        .run_transaction(|conn| {
            ownership::claim(
                conn,
                "session-live-peer",
                "turn-live-peer",
                PREVIOUS_BOOT,
                Some("attach-peer"),
                0,
                Utc::now(),
            )
        })
        .await
        .unwrap();

    assert!(fixture.reconcile().await.is_empty());
    assert_eq!(fixture.session("session-live-peer").await.status, "running");
    assert_eq!(fixture.run("turn-live-peer").await.status, "running");
}

#[tokio::test]
async fn an_expired_lease_is_reconciled_even_for_this_process() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-205", "turn-205", "Run seed 205")
        .await;
    let claimed_at = Utc::now();
    fixture
        .db()
        .run_transaction(move |conn| {
            ownership::claim(
                conn,
                "session-205",
                "turn-205",
                CURRENT_BOOT,
                Some("attach-3"),
                0,
                claimed_at,
            )
        })
        .await
        .unwrap();

    // Still inside the lease: nothing changes.
    assert!(fixture
        .reconcile_at(claimed_at + Duration::seconds(1))
        .await
        .is_empty());

    let notices = fixture
        .reconcile_at(claimed_at + LEASE_DURATION + Duration::seconds(1))
        .await;

    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].reason, RecoveryReason::LeaseExpired.as_str());
    assert_eq!(fixture.session("session-205").await.status, "interrupted");
}

#[tokio::test]
async fn heartbeats_hold_a_long_turn_open() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-206", "turn-206", "Think for a long time")
        .await;
    let start = Utc::now();
    fixture
        .db()
        .run_transaction(move |conn| {
            ownership::claim(
                conn,
                "session-206",
                "turn-206",
                CURRENT_BOOT,
                None,
                0,
                start,
            )
        })
        .await
        .unwrap();

    // XHigh reasoning far outlives one lease; periodic activity must keep it.
    let mut now = start;
    for _ in 0..10 {
        now = now + LEASE_DURATION - Duration::seconds(2);
        let at = now;
        let refreshed = fixture
            .db()
            .run_transaction(move |conn| {
                ownership::heartbeat(conn, "session-206", CURRENT_BOOT, None, at)
            })
            .await
            .unwrap();
        assert!(refreshed);
        assert!(fixture.reconcile_at(now).await.is_empty());
    }
    assert_eq!(fixture.session("session-206").await.status, "running");
}

#[tokio::test]
async fn a_foreign_process_cannot_refresh_a_claim_it_does_not_hold() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-207", "turn-207", "Run seed 207")
        .await;

    let refreshed = fixture
        .db()
        .run_transaction(|conn| {
            ownership::heartbeat(conn, "session-207", CURRENT_BOOT, None, Utc::now())
        })
        .await
        .unwrap();

    assert!(!refreshed);
    assert_eq!(fixture.reconcile().await.len(), 1);
}

#[tokio::test]
async fn recovery_preserves_the_prompt_model_thread_and_history() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn(
            "session-208",
            "turn-208",
            "Run the full Craftax eval for seed 208",
        )
        .await;

    let notice = fixture.reconcile().await.remove(0);

    let prompt = notice.last_user_message.expect("prompt preserved");
    assert_eq!(prompt.text, "Run the full Craftax eval for seed 208");
    assert_eq!(prompt.client_message_id.as_deref(), Some("user-1"));

    let session = fixture.session("session-208").await;
    assert_eq!(session.codex_thread_id.as_deref(), Some("thread-201"));
    assert_eq!(session.target.model(), Some("gpt-5.6-luna"));

    // The interrupted attempt stays in history with its original inputs.
    let run = fixture.run("turn-208").await;
    assert_eq!(run.model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(run.metadata["effort"], "xhigh");
    assert_eq!(run.metadata["threadId"], "thread-201");
}

#[tokio::test]
async fn a_pending_notice_is_readable_and_clears_on_the_next_claim() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-209", "turn-209", "Run seed 209")
        .await;
    fixture.reconcile().await;

    let pending = fixture
        .db()
        .run(|conn| pending_recovery_notices(conn))
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending["session-209"].run_id.as_deref(), Some("turn-209"));

    let cleared = fixture
        .db()
        .run_transaction(|conn| clear_recovery_metadata(conn, "session-209"))
        .await
        .unwrap();
    assert_eq!(cleared, Some(1));

    let pending = fixture
        .db()
        .run(|conn| pending_recovery_notices(conn))
        .await
        .unwrap();
    assert!(pending.is_empty());
    // Session identity survives the metadata edit.
    assert_eq!(
        fixture.session("session-209").await.metadata["model"],
        "gpt-5.6-luna"
    );
}

#[tokio::test]
async fn restarting_creates_a_new_attempt_and_never_reopens_the_old_run() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-210", "turn-210", "Run seed 210")
        .await;
    fixture.reconcile().await;

    // What `CodexManager::start_turn_inner` does on the restart path.
    let previous_attempt = fixture
        .db()
        .run_transaction(|conn| clear_recovery_metadata(conn, "session-210"))
        .await
        .unwrap()
        .unwrap();
    fixture
        .runs
        .start(RunCreate {
            id: "turn-210-retry".into(),
            session_id: "session-210".into(),
            mode: "codex_turn".into(),
            model: Some("gpt-5.6-luna".into()),
            adapter: None,
            metadata: serde_json::json!({
                "recoveryAttempt": previous_attempt,
                "recoveredFromRunId": "turn-210",
                "recoveredAfterCrash": true,
            }),
            source: EventSource::Codex,
        })
        .await
        .unwrap();

    let session = fixture.session("session-210").await;
    assert_eq!(session.status, "running");
    assert_eq!(session.active_run_id.as_deref(), Some("turn-210-retry"));
    // The crashed attempt is preserved exactly as it ended.
    assert_eq!(fixture.run("turn-210").await.status, "interrupted");
    let retry = fixture.run("turn-210-retry").await;
    assert_eq!(retry.metadata["recoveredFromRunId"], "turn-210");
    assert_eq!(retry.metadata["recoveryAttempt"], 1);
}

#[tokio::test]
async fn an_unsettled_external_action_makes_recovery_need_attention() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-211", "turn-211", "Launch the seed 211 rollout")
        .await;
    fixture
        .db()
        .run_transaction(|conn| {
            receipts::begin(
                conn,
                "session-211",
                Some("turn-211"),
                "rollout:craftax-seed-211",
                "container.rollout.start",
                &serde_json::json!({"rollout_id": "craftax-seed-211"}),
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let notice = fixture.reconcile().await.remove(0);

    assert!(!notice.restartable);
    assert!(notice.needs_attention);
}

#[tokio::test]
async fn a_settled_rollout_is_reattached_not_relaunched() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-212", "turn-212", "Launch the seed 212 rollout")
        .await;
    fixture
        .db()
        .run_transaction(|conn| {
            let receipt = receipts::begin(
                conn,
                "session-212",
                Some("turn-212"),
                "rollout:craftax-seed-212",
                "container.rollout.start",
                &serde_json::json!({"rollout_id": "craftax-seed-212"}),
            )?;
            receipts::settle(conn, &receipt.tool_call_id, Some("craftax-seed-212"))
        })
        .await
        .unwrap();

    let notice = fixture.reconcile().await.remove(0);

    assert!(!notice.restartable);
    assert!(!notice.needs_attention);
    assert_eq!(
        notice.external_object_id.as_deref(),
        Some("craftax-seed-212")
    );
}

#[tokio::test]
async fn a_receipt_for_the_same_action_is_never_opened_twice() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-213", "turn-213", "Launch the seed 213 rollout")
        .await;
    let request = serde_json::json!({"rollout_id": "craftax-seed-213", "seed": 213});

    let (first, second) = fixture
        .db()
        .run_transaction(move |conn| {
            let first = receipts::begin(
                conn,
                "session-213",
                Some("turn-213"),
                "rollout:craftax-seed-213",
                "container.rollout.start",
                &request,
            )?;
            let second = receipts::begin(
                conn,
                "session-213",
                Some("turn-213"),
                "rollout:craftax-seed-213",
                "container.rollout.start",
                &request,
            )?;
            Ok((first, second))
        })
        .await
        .unwrap();

    assert_eq!(first.tool_call_id, second.tool_call_id);
    let stored = fixture
        .db()
        .run(|conn| receipts::for_run(conn, "turn-213"))
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
}

#[tokio::test]
async fn a_failed_action_still_allows_a_restart() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-214", "turn-214", "Launch the seed 214 rollout")
        .await;
    fixture
        .db()
        .run_transaction(|conn| {
            let receipt = receipts::begin(
                conn,
                "session-214",
                Some("turn-214"),
                "rollout:craftax-seed-214",
                "container.rollout.start",
                &serde_json::json!({"rollout_id": "craftax-seed-214"}),
            )?;
            receipts::fail(conn, &receipt.tool_call_id)
        })
        .await
        .unwrap();

    let notice = fixture.reconcile().await.remove(0);

    assert!(notice.restartable);
    assert!(!notice.needs_attention);
}

#[tokio::test]
async fn five_parallel_chats_all_recover() {
    let fixture = Fixture::new();
    for seed in 201..=205 {
        fixture
            .abandoned_turn(
                &format!("session-{seed}"),
                &format!("turn-{seed}"),
                &format!("Run the Craftax eval for seed {seed}"),
            )
            .await;
    }

    let notices = fixture.reconcile().await;

    assert_eq!(notices.len(), 5);
    for seed in 201..=205 {
        let session = fixture.session(&format!("session-{seed}")).await;
        assert_eq!(session.status, "interrupted", "session-{seed}");
        assert!(session.active_run_id.is_none(), "session-{seed}");
        assert_eq!(
            fixture.run(&format!("turn-{seed}")).await.status,
            "interrupted",
            "turn-{seed}"
        );
    }
    // Nothing is left claiming to be live.
    let owned = fixture
        .db()
        .run(|conn| ownership::owned_sessions(conn, PREVIOUS_BOOT))
        .await
        .unwrap();
    assert!(owned.is_empty());
}

#[tokio::test]
async fn a_running_run_whose_session_moved_on_is_still_closed() {
    let fixture = Fixture::new();
    fixture
        .abandoned_turn("session-215", "turn-215", "Run seed 215")
        .await;
    // The exact shape a partial crash leaves: the session settled, the run row
    // never did.
    fixture
        .db()
        .run_transaction(|conn| {
            conn.execute(
                "UPDATE sessions SET status = 'ready', active_run_id = NULL WHERE id = 'session-215'",
                [],
            )?;
            ownership::release(conn, "session-215")
        })
        .await
        .unwrap();

    fixture.reconcile().await;

    assert_eq!(fixture.run("turn-215").await.status, "interrupted");
}

#[test]
fn a_claim_is_live_for_this_owner_or_a_fresh_peer() {
    let now = Utc::now();
    let claim = ownership::TurnClaim {
        session_id: "session-1".into(),
        run_id: "turn-1".into(),
        owner_instance_id: CURRENT_BOOT.into(),
        owner_attachment_id: None,
        claimed_at: now.to_rfc3339(),
        heartbeat_at: now.to_rfc3339(),
        lease_expires_at: (now + LEASE_DURATION).to_rfc3339(),
        recovery_attempt: 0,
        last_checkpoint: None,
    };

    assert!(claim.is_live(CURRENT_BOOT, now));
    // A live peer is still live: the second process must not interrupt it.
    assert!(claim.is_live(PREVIOUS_BOOT, now));
    assert!(!claim.is_live(CURRENT_BOOT, now + LEASE_DURATION + Duration::seconds(1)));
    assert!(!claim.is_live(PREVIOUS_BOOT, now + LEASE_DURATION + Duration::seconds(1)));

    // An unparseable lease is not evidence of liveness for the owner.
    let corrupt = ownership::TurnClaim {
        lease_expires_at: "not-a-timestamp".into(),
        ..claim.clone()
    };
    assert!(!corrupt.is_live(CURRENT_BOOT, now));

    // An unparseable heartbeat is not evidence of a live peer.
    let corrupt_heartbeat = ownership::TurnClaim {
        heartbeat_at: "not-a-timestamp".into(),
        ..claim
    };
    assert!(!corrupt_heartbeat.is_live(PREVIOUS_BOOT, now));
}

#[test]
fn optimizer_run_claim_is_live_only_for_its_owner_and_only_before_it_expires() {
    let fx = Fixture::new();
    let now = Utc::now();
    fx.db()
        .with_conn(|conn| {
            ownership::claim_optimizer_run(
                conn,
                "opt_1",
                CURRENT_BOOT,
                CURRENT_BOOT,
                Some(1),
                None,
                now,
            )?;
            assert!(ownership::optimizer_run_is_live(
                conn,
                "opt_1",
                CURRENT_BOOT,
                now
            )?);
            assert!(!ownership::optimizer_run_is_live(
                conn,
                "opt_1",
                PREVIOUS_BOOT,
                now
            )?);
            assert!(!ownership::claim_is_live(
                conn,
                ownership::KIND_OPTIMIZER_RUN,
                "opt_1",
                PREVIOUS_BOOT,
                now
            )?);
            assert!(ownership::claim_is_live(
                conn,
                ownership::KIND_OPTIMIZER_RUN,
                "opt_1",
                CURRENT_BOOT,
                now
            )?);
            Ok(())
        })
        .unwrap();
}
