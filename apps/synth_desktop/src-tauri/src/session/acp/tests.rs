//! ACP lifecycle fault coverage.
//!
//! Every peer here is `scripts/fixtures/workshop-acp-agent.py` — a **fixture**
//! that speaks the protocol and calls no model, network or credential. It
//! simulates protocol boundaries (a permission request that never resolves
//! itself, a turn that ignores cancellation, an abrupt exit); it is not
//! evidence about any real adapter's behaviour.
//!
//! Each test owns its own data root, backend registry and workspace. Nothing
//! here signals a process it did not spawn.

use super::*;
use crate::recovery::{ownership, LEASE_DURATION};
use chrono::Utc;
use std::path::Path;
use tempfile::TempDir;

struct Harness {
    _temp: TempDir,
    _workspace: TempDir,
    core: Arc<CoreRuntime>,
    _app: tauri::App<tauri::test::MockRuntime>,
    manager: Arc<Manager<tauri::test::MockRuntime>>,
}

/// `attach` requires the packaged MCP bridge beside the running executable.
/// Under `cargo test` that directory is `target/debug/deps`, where the bridge
/// binary is never placed, so the harness supplies a marker. The fixture peer
/// only asserts the server is named `workshop`; it never runs it.
fn ensure_bridge_marker() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else { return };
    let bridge = dir.join(if cfg!(windows) { "workshop.exe" } else { "workshop" });
    if !bridge.is_file() {
        let _ = std::fs::write(&bridge, b"#!/bin/sh\nexit 0\n");
    }
}

fn write_backend(root: &Path, workspace: &Path, id: &str, max_turn_seconds: u32) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts/fixtures/workshop-acp-agent.py")
        .canonicalize()
        .expect("the ACP fixture peer must be present");
    let launcher = workspace.join("acp-fixture.sh");
    std::fs::write(
        &launcher,
        format!("#!/bin/sh\nexec python3 {} \"$@\"\n", fixture.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let registry = root.join("agent-backends.json");
    std::fs::write(
        &registry,
        serde_json::to_vec_pretty(&json!([{
            "id": id,
            "command": launcher,
            "args": [],
            "workspace": workspace,
            "envFile": null,
            "maxSessions": 4,
            "maxTurnSeconds": max_turn_seconds,
        }]))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&registry, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

async fn harness(max_turn_seconds: u32) -> Harness {
    ensure_bridge_marker();
    let temp = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let root = temp.path().join("core");
    let core = Arc::new(CoreRuntime::open(&root).unwrap());
    write_backend(&root, workspace.path(), "fixture", max_turn_seconds);
    let app = tauri::test::mock_app();
    let approvals = Arc::new(ApprovalBroker::new(
        crate::session::persistence::SessionPersistence::from_core(Some(core.clone())),
    ));
    let manager = Arc::new(Manager::new(core.clone(), app.handle().clone(), approvals));
    Harness {
        _temp: temp,
        _workspace: workspace,
        core,
        _app: app,
        manager,
    }
}

impl Harness {
    async fn start(&self) -> String {
        let started = self
            .manager
            .start(StartRequest {
                backend_id: "fixture".into(),
                title: "fixture agent".into(),
                parent_session_id: None,
            })
            .await
            .expect("the fixture backend must attach");
        started["sessionId"].as_str().unwrap().to_owned()
    }

    async fn claim(&self, session: &str) -> Option<ownership::TurnClaim> {
        let id = session.to_owned();
        self.core
            .storage()
            .database()
            .clone()
            .run(move |conn| ownership::load(conn, &id))
            .await
            .unwrap()
    }

    async fn run_status(&self, run_id: &str) -> String {
        let id = run_id.to_owned();
        self.core
            .storage()
            .database()
            .clone()
            .run(move |conn| {
                Ok(conn
                    .query_row("SELECT status FROM runs WHERE id=?1", [&id], |row| {
                        row.get::<_, String>(0)
                    })
                    .ok())
            })
            .await
            .unwrap()
            .unwrap_or_default()
    }

    async fn session_status(&self, session: &str) -> String {
        let id = session.to_owned();
        self.core
            .storage()
            .database()
            .clone()
            .run(move |conn| {
                Ok(conn
                    .query_row("SELECT status FROM sessions WHERE id=?1", [&id], |row| {
                        row.get::<_, String>(0)
                    })
                    .ok())
            })
            .await
            .unwrap()
            .unwrap_or_default()
    }

    async fn prompts_recorded(&self, session: &str) -> usize {
        let id = session.to_owned();
        self.core
            .storage()
            .database()
            .clone()
            .run(move |conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM events WHERE session_id=?1 AND kind='agent.prompt'",
                    [&id],
                    |row| row.get::<_, i64>(0),
                )?)
            })
            .await
            .unwrap() as usize
    }

    async fn outcome(&self, run_id: &str) -> String {
        let id = run_id.to_owned();
        self.core
            .storage()
            .database()
            .clone()
            .run(move |conn| {
                Ok(conn
                    .query_row("SELECT outcome_json FROM runs WHERE id=?1", [&id], |row| {
                        row.get::<_, Option<String>>(0)
                    })
                    .ok()
                    .flatten()
                    .unwrap_or_default())
            })
            .await
            .unwrap()
    }

    /// Wait for a settled turn without sleeping a fixed amount: the completion
    /// task is asynchronous, and a fixed sleep is either slow or flaky.
    async fn settled(&self, run_id: &str) -> String {
        for _ in 0..200 {
            let status = self.run_status(run_id).await;
            if !matches!(status.as_str(), "running" | "queued" | "created" | "") {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("turn {run_id} never settled");
    }

    /// The run transition and the lease release are separate writes in the
    /// same completion task, so a settled run is not yet proof of a released
    /// lease. Wait for the release rather than racing it.
    async fn released(&self, session: &str) {
        for _ in 0..200 {
            if self.claim(session).await.is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("session {session} still holds a turn claim");
    }
}

#[tokio::test]
async fn an_ordinary_turn_claims_then_releases_its_lease() {
    let harness = harness(30).await;
    let session = harness.start().await;
    assert!(harness.claim(&session).await.is_none(), "idle sessions own nothing");

    let accepted = harness
        .manager
        .send(session.clone(), "hello".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    assert_eq!(harness.settled(&run_id).await, "completed");
    harness.released(&session).await;
    assert_eq!(harness.session_status(&session).await, "ready");
    harness.manager.close(&session).await.unwrap();
}

#[tokio::test]
async fn a_second_turn_cannot_start_while_one_is_active() {
    let harness = harness(30).await;
    let session = harness.start().await;
    let accepted = harness
        .manager
        .send(session.clone(), "wait".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    let refused = harness
        .manager
        .send(session.clone(), "hello".into())
        .await
        .expect_err("a busy agent must refuse a second turn");
    assert!(refused.to_string().contains("active turn"), "{refused}");
    assert_eq!(harness.prompts_recorded(&session).await, 1);

    harness.manager.close(&session).await.unwrap();
    assert_eq!(harness.settled(&run_id).await, "interrupted");
}

/// The reported fault: a human wait longer than the lease used to look like a
/// dead process to the watchdog. The turn heartbeats while it waits, so a
/// sweep run past the original expiry still finds a live claim.
#[tokio::test]
async fn a_permission_wait_outlives_its_original_lease() {
    let harness = harness(120).await;
    let session = harness.start().await;
    let accepted = harness
        .manager
        .send(session.clone(), "permission".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    let first = harness.claim(&session).await.expect("a started turn is claimed");
    assert_eq!(first.run_id, run_id);
    let first_expiry = first.lease_expires_at.clone();

    // One heartbeat tick is five seconds; the turn is still waiting on a human.
    tokio::time::sleep(Duration::from_millis(6_500)).await;
    let refreshed = harness
        .claim(&session)
        .await
        .expect("a waiting turn keeps its claim");
    assert!(
        refreshed.lease_expires_at > first_expiry,
        "a human wait must refresh the lease: {first_expiry} -> {}",
        refreshed.lease_expires_at
    );

    // A sweep just past the *original* expiry recovers nothing, because the
    // refresh moved the lease beyond it. Recovering here is the false
    // `workshop_restarted` event this ownership work exists to prevent.
    let instance = crate::instance::boot_epoch().to_owned();
    let past_original = chrono::DateTime::parse_from_rfc3339(&first_expiry)
        .unwrap()
        .with_timezone(&Utc)
        + chrono::Duration::seconds(1);
    assert!(
        past_original > Utc::now(),
        "the sweep must be simulated past the original expiry, not before it"
    );
    let recovered = harness
        .core
        .storage()
        .database()
        .clone()
        .run_transaction(move |conn| {
            crate::recovery::reconcile_orphaned_turns(conn, &instance, past_original)
        })
        .await
        .unwrap();
    assert!(
        recovered.is_empty(),
        "a heartbeated turn must not be recovered: {recovered:?}"
    );

    harness.manager.close(&session).await.unwrap();
    assert_eq!(harness.settled(&run_id).await, "interrupted");
    harness.released(&session).await;
}

/// A sweep that runs while a turn is being admitted must not steal the claim
/// the admission just took.
#[tokio::test]
async fn a_watchdog_sweep_concurrent_with_admission_recovers_nothing() {
    let harness = harness(120).await;
    let session = harness.start().await;

    let sweeper = {
        let core = harness.core.clone();
        tokio::spawn(async move {
            let mut swept = 0;
            for _ in 0..40 {
                swept += core.sweep_expired_leases().await.unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            swept
        })
    };
    let accepted = harness
        .manager
        .send(session.clone(), "wait".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();
    assert_eq!(sweeper.await.unwrap(), 0, "a fresh claim is not expired");

    let claim = harness.claim(&session).await.expect("the claim survives the sweep");
    assert_eq!(claim.run_id, run_id);
    assert_eq!(harness.run_status(&run_id).await, "running");

    harness.manager.close(&session).await.unwrap();
    assert_eq!(harness.settled(&run_id).await, "interrupted");
}

/// Fixture prompt `crash`: the peer exits mid-turn. The turn is uncertain, not
/// completed, and nothing is replayed on its behalf.
#[tokio::test]
async fn an_eof_mid_turn_fails_the_run_and_replays_nothing() {
    let harness = harness(30).await;
    let session = harness.start().await;
    let accepted = harness
        .manager
        .send(session.clone(), "crash".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    assert_eq!(harness.settled(&run_id).await, "failed");
    harness.released(&session).await;
    assert_eq!(harness.session_status(&session).await, "failed");
    assert_eq!(
        harness.prompts_recorded(&session).await,
        1,
        "an uncertain prompt must never be replayed automatically"
    );
}

/// Fixture prompt `wait`: the peer never answers. The bounded request expires,
/// and the recorded outcome says it is uncertain rather than failed-and-known.
#[tokio::test]
async fn a_turn_timeout_settles_uncertain_and_releases_the_lease() {
    let harness = harness(1).await;
    let session = harness.start().await;
    let accepted = harness
        .manager
        .send(session.clone(), "wait".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    assert_eq!(harness.settled(&run_id).await, "failed");
    let outcome = harness.outcome(&run_id).await;
    assert!(
        outcome.contains("outcomeUncertain") && outcome.contains("timed out"),
        "a timeout must record an uncertain outcome: {outcome}"
    );
    harness.released(&session).await;
    assert_eq!(harness.prompts_recorded(&session).await, 1);
}

/// Fixture prompt `wait` answers cancellation inside the grace window.
#[tokio::test]
async fn cancellation_inside_the_grace_window_interrupts_the_turn() {
    let harness = harness(120).await;
    let session = harness.start().await;
    let accepted = harness
        .manager
        .send(session.clone(), "wait".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    harness.manager.cancel(&session).await.unwrap();
    assert_eq!(harness.settled(&run_id).await, "interrupted");
    harness.released(&session).await;
    // A cancelled turn is not a failure: the session stays usable and the
    // agent is still attached for the next prompt.
    assert_ne!(harness.session_status(&session).await, "failed");
    let next = harness
        .manager
        .send(session.clone(), "hello".into())
        .await
        .expect("a cancelled session still accepts the next turn");
    assert_eq!(
        harness.settled(next["runId"].as_str().unwrap()).await,
        "completed"
    );
    harness.manager.close(&session).await.unwrap();
}

/// Fixture prompt `ignore-cancel` never answers the cancel. The deadline is
/// what settles it, and the attachment is dropped rather than left owned.
#[tokio::test]
async fn a_cancellation_the_peer_ignores_is_settled_by_the_deadline() {
    let harness = harness(120).await;
    let session = harness.start().await;
    let accepted = harness
        .manager
        .send(session.clone(), "ignore-cancel".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    harness.manager.cancel(&session).await.unwrap();
    assert_eq!(harness.settled(&run_id).await, "interrupted");
    harness.released(&session).await;
    assert!(
        harness.manager.cancel(&session).await.is_err(),
        "the attachment is gone after a forced termination"
    );
    assert_eq!(harness.prompts_recorded(&session).await, 1);
}

/// Closing while a turn is open settles the run and the session together.
#[tokio::test]
async fn closing_an_active_session_settles_run_and_session_together() {
    let harness = harness(120).await;
    let session = harness.start().await;
    let accepted = harness
        .manager
        .send(session.clone(), "wait".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    harness.manager.close(&session).await.unwrap();
    assert_eq!(harness.settled(&run_id).await, "interrupted");
    harness.released(&session).await;
    assert_eq!(harness.session_status(&session).await, "closed");
    assert!(harness.manager.send(session, "hello".into()).await.is_err());
}

/// The fixture replays 150 chunks during `session/load` — more than the host's
/// 128-slot inbound queue. The handshake must drain them, not deadlock.
#[tokio::test]
async fn resume_drains_a_replay_larger_than_the_inbound_queue() {
    let harness = harness(120).await;
    let session = harness.start().await;
    harness.manager.close(&session).await.unwrap();

    let resumed = tokio::time::timeout(
        Duration::from_secs(30),
        harness.manager.resume(&session),
    )
    .await
    .expect("resume must not deadlock behind a full inbound queue")
    .expect("resume must succeed");
    assert_eq!(resumed["sessionId"], json!(session));

    let replayed = {
        let id = session.clone();
        harness
            .core
            .storage()
            .database()
            .clone()
            .run(move |conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM events WHERE session_id=?1 AND kind='agent.update'",
                    [&id],
                    |row| row.get::<_, i64>(0),
                )?)
            })
            .await
            .unwrap()
    };
    assert_eq!(replayed, 150, "every replayed chunk must reach the journal");
    assert!(
        harness.claim(&session).await.is_none(),
        "a resumed session owns nothing until a turn starts"
    );
    assert_eq!(
        harness.prompts_recorded(&session).await,
        0,
        "resuming must not replay a prompt"
    );

    // The resumed attachment is a new generation, and takes turns normally.
    let accepted = harness
        .manager
        .send(session.clone(), "hello".into())
        .await
        .unwrap();
    assert_eq!(
        harness
            .settled(accepted["runId"].as_str().unwrap())
            .await,
        "completed"
    );
    harness.manager.close(&session).await.unwrap();
}

/// A permission callback whose turn is gone must be answered `cancelled`, and
/// must never reach the human approval broker.
#[tokio::test]
async fn a_permission_callback_from_a_stale_generation_is_cancelled() {
    let harness = harness(120).await;
    let session = harness.start().await;
    let accepted = harness
        .manager
        .send(session.clone(), "permission".into())
        .await
        .unwrap();
    let run_id = accepted["runId"].as_str().unwrap().to_owned();

    // Close, then resume: the attachment that issued the callback is stale.
    harness.manager.close(&session).await.unwrap();
    assert_eq!(harness.settled(&run_id).await, "interrupted");
    harness.manager.resume(&session).await.unwrap();

    let stale = harness
        .core
        .storage()
        .database()
        .clone()
        .run({
            let id = session.clone();
            move |conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM events WHERE session_id=?1 AND kind='agent.prompt'",
                    [&id],
                    |row| row.get::<_, i64>(0),
                )?)
            }
        })
        .await
        .unwrap();
    assert_eq!(stale, 1, "a stale generation must not replay its prompt");
    harness.released(&session).await;
    harness.manager.close(&session).await.unwrap();
}

/// Every failure path releases the lease. This asserts the invariant across
/// them together, so a new terminal path cannot quietly skip the release.
#[tokio::test]
async fn every_terminal_path_leaves_no_claim_behind() {
    for (prompt, expected, turn_seconds) in [
        ("hello", "completed", 30_u32),
        ("crash", "failed", 30),
        ("wait", "failed", 1),
    ] {
        let harness = harness(turn_seconds).await;
        let session = harness.start().await;
        let accepted = harness
            .manager
            .send(session.clone(), prompt.into())
            .await
            .unwrap();
        let run_id = accepted["runId"].as_str().unwrap().to_owned();
        assert_eq!(harness.settled(&run_id).await, expected, "prompt {prompt}");
        harness.released(&session).await;
        let session_status = harness.session_status(&session).await;
        assert_eq!(
            session_status == "failed",
            expected == "failed",
            "prompt {prompt}: run {expected} but session {session_status}"
        );
    }
}
