use super::*;
use crate::storage::Storage;
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
fn stream() -> Stream {
    Stream {
        adapter: Adapter::InternSync,
        external_id: "runtime-1".into(),
    }
}
fn setup() -> (TempDir, Arc<Database>, CloudStore, ScopeLease, String) {
    let dir = tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let db = storage.database().clone();
    db.transaction(|conn| {
        conn.execute_batch(MIGRATION_CANDIDATE)?;
        Ok(())
    })
    .unwrap();
    let store = CloudStore::open(db.clone()).unwrap();
    let lease = store.activate_verified(&identity("a")).unwrap();
    let session = store
        .create_conversation(&lease, &stream(), "fixture")
        .unwrap();
    (dir, db, store, lease, session)
}
fn intent() -> CommandIntent {
    CommandIntent {
        command_id: "cmd-1".into(),
        stream: stream(),
        operation_id: "send".into(),
        idempotency_key: "key-1".into(),
        body: br#"{"body":"fixture","expected_generation":0}"#.to_vec(),
        expected_generation: Some(0),
    }
}
fn event(id: &str, seq: u64) -> RemoteEvent {
    RemoteEvent {
        id: id.into(),
        kind: "agent_message".into(),
        payload: json!({"body":"fixture"}),
        sequence: Some(seq),
        generation: Some(0),
    }
}

#[test]
fn schema_is_not_automatically_installed_and_upgrade_rolls_back() {
    let dir = tempdir().unwrap();
    let db = Storage::open(dir.path()).unwrap().database().clone();
    assert!(CloudStore::open(db.clone()).is_err());
    db.with_conn(|conn| { conn.execute("INSERT INTO sessions(id,title,target_json,status,created_at,updated_at) VALUES('legacy','legacy','{}','ready','now','now')",[])?;Ok(()) }).unwrap();
    let failed: Result<()> = db.transaction(|conn| {
        conn.execute_batch(MIGRATION_CANDIDATE)?;
        bail!("simulated upgrade failure")
    });
    assert!(failed.is_err());
    assert!(CloudStore::open(db.clone()).is_err());
    db.transaction(|conn| {
        conn.execute_batch(MIGRATION_CANDIDATE)?;
        Ok(())
    })
    .unwrap();
    let store = CloudStore::open(db.clone()).unwrap();
    let lease = store.activate_verified(&identity("a")).unwrap();
    assert!(store.bound_sessions(&lease).unwrap().is_empty());
    assert!(store.bind_session(&lease, &stream(), "legacy").is_err());
    db.with_conn(|conn| {
        let title: String =
            conn.query_row("SELECT title FROM sessions WHERE id='legacy'", [], |r| {
                r.get(0)
            })?;
        assert_eq!(title, "legacy");
        Ok(())
    })
    .unwrap();
}

#[test]
fn account_switch_and_boot_fence_reads_writes_and_pending_commands() {
    let (_dir, db, store, a, session_a) = setup();
    store.enqueue(&a, &intent()).unwrap();
    let b = store.activate_verified(&identity("b")).unwrap();
    assert!(store.bound_sessions(&a).is_err());
    assert!(store.begin_send(&a, "cmd-1").is_err());
    assert!(store.bound_sessions(&b).unwrap().is_empty());
    assert!(store.command(&b, "cmd-1").is_err());
    let session_b = store.create_conversation(&b, &stream(), "B").unwrap();
    assert_ne!(session_a, session_b);
    let a2 = store.activate_verified(&identity("a")).unwrap();
    assert_eq!(store.bound_sessions(&a2).unwrap(), vec![session_a]);
    assert!(store.begin_send(&a2, "cmd-1").is_err()); // old epoch must not flush
    let reopened = CloudStore::open(db).unwrap();
    assert!(reopened.command(&a2, "cmd-1").is_err());
}

#[test]
fn backend_org_profile_and_origin_are_all_part_of_identity() {
    let base = identity("a");
    let key = base.key().unwrap();
    for field in 0..4 {
        let mut changed = base.clone();
        match field {
            0 => changed.backend_id = "other".into(),
            1 => changed.org_id = "other".into(),
            2 => changed.profile_id = "other".into(),
            _ => changed.backend_origin = "https://other.invalid".into(),
        };
        assert_ne!(key, changed.key().unwrap());
    }
    let mut canonical = base.clone();
    canonical.backend_origin.push('/');
    assert_eq!(key, canonical.key().unwrap());
    canonical.backend_origin = "https://user:secret@fixture.invalid".into();
    assert!(canonical.key().is_err());
}

#[test]
fn outbox_preserves_exact_bytes_and_refuses_semantic_key_reuse() {
    let (_dir, db, store, lease, _) = setup();
    let original = intent();
    let queued = store.enqueue(&lease, &original).unwrap();
    assert_eq!(queued, store.enqueue(&lease, &original).unwrap());
    let mut changed = original.clone();
    changed.body = br#"{"body":"different"}"#.to_vec();
    assert!(store.enqueue(&lease, &changed).is_err());
    changed = original.clone();
    changed.expected_generation = Some(1);
    assert!(store.enqueue(&lease, &changed).is_err());
    db.with_conn(|conn| {
        assert!(conn
            .execute("UPDATE cloud_command_outbox SET body='changed'", [])
            .is_err());
        Ok(())
    })
    .unwrap();
    let sent = store.begin_send(&lease, &original.command_id).unwrap();
    assert_eq!(sent.body, original.body);
    assert_eq!(sent.state, "outcome_unknown");
    assert!(store.begin_send(&lease, &original.command_id).is_err());
}

#[test]
fn concurrent_dispatch_claim_has_one_winner() {
    let (_dir, _db, store, lease, _) = setup();
    store.enqueue(&lease, &intent()).unwrap();
    let store = Arc::new(store);
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let lease = lease.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.begin_send(&lease, "cmd-1").is_ok()
            })
        })
        .collect();
    barrier.wait();
    assert_eq!(
        workers
            .into_iter()
            .filter(|w| w.thread().id() != std::thread::current().id())
            .map(|w| usize::from(w.join().unwrap()))
            .sum::<usize>(),
        1
    );
}

#[test]
fn timeout_restart_preserves_request_and_signout_fences_late_receipt() {
    let (_dir, db, store, lease, _) = setup();
    let original = intent();
    store.enqueue(&lease, &original).unwrap();
    store.begin_send(&lease, "cmd-1").unwrap();
    store.sign_out().unwrap();
    assert!(store
        .record_receipt(&lease, "cmd-1", ReceiptStage::Applied, &json!({}))
        .is_err());
    let restarted = CloudStore::open(db).unwrap();
    let fresh = restarted.activate_verified(&identity("a")).unwrap();
    let retained = restarted.command(&fresh, "cmd-1").unwrap();
    assert_eq!(retained.body, original.body);
    assert_eq!(retained.state, "outcome_unknown");
    assert!(restarted.begin_send(&fresh, "cmd-1").is_err());
    // A verified recovery read can update the receipt under the new epoch.
    restarted
        .record_receipt(
            &fresh,
            "cmd-1",
            ReceiptStage::Applied,
            &json!({"reconciled":true}),
        )
        .unwrap();
}

#[test]
fn receipt_stages_are_monotonic_and_do_not_complete_remote_execution() {
    let (_dir, db, store, lease, _) = setup();
    store.bind_execution(&lease, &stream(), "run-1").unwrap();
    store.enqueue(&lease, &intent()).unwrap();
    assert!(store
        .record_receipt(&lease, "cmd-1", ReceiptStage::Received, &json!({}))
        .is_err());
    store.begin_send(&lease, "cmd-1").unwrap();
    for stage in [
        ReceiptStage::Received,
        ReceiptStage::Delivered,
        ReceiptStage::Applied,
    ] {
        store
            .record_receipt(&lease, "cmd-1", stage, &json!({}))
            .unwrap();
    }
    assert!(store
        .record_receipt(&lease, "cmd-1", ReceiptStage::Delivered, &json!({}))
        .is_err());
    assert!(store
        .record_receipt(&lease, "cmd-1", ReceiptStage::Refused, &json!({}))
        .is_err());
    assert!(store
        .record_receipt(
            &lease,
            "cmd-1",
            ReceiptStage::Applied,
            &json!({"changed":true})
        )
        .is_err());
    db.with_conn(|conn| {
        let state: String = conn.query_row(
            "SELECT remote_state FROM cloud_execution_bindings",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(state, "reconciling");
        Ok(())
    })
    .unwrap();
}

#[test]
fn page_checkpoint_and_journal_rollback_together_and_replay_checks_content() {
    let (_dir, db, store, lease, _) = setup();
    let checkpoint = Checkpoint::Intern {
        sequence: 1,
        generation: 0,
    };
    db.with_conn(|conn|{conn.execute_batch("CREATE TRIGGER fail_checkpoint BEFORE INSERT ON cloud_checkpoints BEGIN SELECT RAISE(ABORT,'injected commit failure'); END;")?;Ok(())}).unwrap();
    assert!(store
        .commit_page(&lease, &stream(), None, &checkpoint, &[event("e1", 1)])
        .is_err());
    assert_eq!(store.checkpoint(&lease, &stream()).unwrap(), None);
    db.with_conn(|conn| {
        let count: i64 = conn.query_row("SELECT count(*) FROM cloud_event_bindings", [], |r| {
            r.get(0)
        })?;
        assert_eq!(count, 0);
        conn.execute_batch("DROP TRIGGER fail_checkpoint")?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        store
            .commit_page(&lease, &stream(), None, &checkpoint, &[event("e1", 1)])
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .commit_page(
            &lease,
            &stream(),
            Some(&checkpoint),
            &checkpoint,
            &[event("e1", 1)]
        )
        .unwrap()
        .is_empty());
    let mut forged = event("e1", 1);
    forged.payload = json!({"different":true});
    assert!(store
        .commit_page(&lease, &stream(), Some(&checkpoint), &checkpoint, &[forged])
        .is_err());
    assert!(store
        .commit_page(&lease, &stream(), None, &checkpoint, &[])
        .is_err());
}

#[test]
fn separate_cursor_adapters_and_account_event_ids_never_alias() {
    let (_dir, _db, store, a, session) = setup();
    let swarm = Stream {
        adapter: Adapter::Swarm,
        external_id: "swarm".into(),
    };
    let mq = Stream {
        adapter: Adapter::Mq,
        external_id: "thread".into(),
    };
    store.bind_session(&a, &swarm, &session).unwrap();
    store.bind_session(&a, &mq, &session).unwrap();
    let swarm_checkpoint = Checkpoint::Swarm {
        event_id: Some("opaque:99-x".into()),
        state_version: None,
        transcript_cursor: Some("archive:A".into()),
    };
    let mut e = event("shared", 1);
    e.sequence = None;
    e.generation = None;
    store
        .commit_page(&a, &swarm, None, &swarm_checkpoint, &[e])
        .unwrap();
    let wake = Checkpoint::Mq {
        subscription_id: "sub".into(),
        sequence: 1,
    };
    assert!(store.commit_page(&a, &mq, None, &wake, &[]).is_err());
    store
        .commit_page(&a, &mq, None, &wake, &[event("shared", 1)])
        .unwrap();
    let intern = Checkpoint::Intern {
        sequence: 1,
        generation: 0,
    };
    let first = store
        .commit_page(&a, &stream(), None, &intern, &[event("shared", 1)])
        .unwrap();
    let b = store.activate_verified(&identity("b")).unwrap();
    store.create_conversation(&b, &stream(), "B").unwrap();
    let second = store
        .commit_page(&b, &stream(), None, &intern, &[event("shared", 1)])
        .unwrap();
    assert_ne!(first[0].event_id, second[0].event_id);
}

#[test]
fn scoped_execution_owner_survives_remount_and_is_hidden_after_switch() {
    let (_dir, _db, store, a, session) = setup();
    store.bind_execution(&a, &stream(), "remote-run").unwrap();
    assert_eq!(
        store
            .execution_owner(&a, Adapter::InternSync, "remote-run")
            .unwrap(),
        Some(session.clone())
    );
    store.bind_execution(&a, &stream(), "remote-run").unwrap();
    let b = store.activate_verified(&identity("b")).unwrap();
    assert_eq!(
        store
            .execution_owner(&b, Adapter::InternSync, "remote-run")
            .unwrap(),
        None
    );
    let a2 = store.activate_verified(&identity("a")).unwrap();
    assert_eq!(
        store
            .execution_owner(&a2, Adapter::InternSync, "remote-run")
            .unwrap(),
        Some(session)
    );
}

#[tokio::test]
async fn dispatch_commits_before_network_and_timeout_never_creates_new_identity() {
    let (_dir, _db, store, lease, _) = setup();
    let original = intent();
    let result = store
        .dispatch_once(&lease, &original, |request| {
            assert_eq!(
                store.command(&lease, "cmd-1").unwrap().state,
                "outcome_unknown"
            );
            assert_eq!(request.body, original.body);
            async { bail!("simulated timeout after remote acceptance") }
        })
        .await;
    assert!(result.is_err());
    let retained = store.command(&lease, "cmd-1").unwrap();
    assert_eq!(retained.idempotency_key, original.idempotency_key);
    assert_eq!(retained.state, "outcome_unknown");
    assert!(store
        .dispatch_once(&lease, &original, |_| async {
            panic!("must not retry unknown command")
        })
        .await
        .is_err());
}

#[tokio::test]
async fn dispatch_rejects_wrong_receipt_and_account_switch_during_send() {
    for switch in [false, true] {
        let (_dir, _db, store, lease, _) = setup();
        let result = store
            .dispatch_once(&lease, &intent(), |_| async {
                if switch {
                    store.sign_out().unwrap();
                }
                Ok(DeliveryReceipt {
                    command_id: if switch { "cmd-1" } else { "wrong-command" }.into(),
                    stream: stream(),
                    stage: ReceiptStage::Applied,
                    detail: json!({}),
                })
            })
            .await;
        assert!(result.is_err());
        let fresh = store.activate_verified(&identity("a")).unwrap();
        assert_eq!(
            store.command(&fresh, "cmd-1").unwrap().state,
            "outcome_unknown"
        );
    }
}

#[test]
fn numeric_replay_rejects_unknown_old_ids_and_duplicate_page_sequences() {
    let (_dir, _db, store, lease, _) = setup();
    let one = Checkpoint::Intern {
        sequence: 1,
        generation: 0,
    };
    store
        .commit_page(&lease, &stream(), None, &one, &[event("e1", 1)])
        .unwrap();
    assert!(store
        .commit_page(
            &lease,
            &stream(),
            Some(&one),
            &one,
            &[event("forged-old", 1)]
        )
        .is_err());
    let two = Checkpoint::Intern {
        sequence: 2,
        generation: 0,
    };
    assert!(store
        .commit_page(
            &lease,
            &stream(),
            Some(&one),
            &two,
            &[event("e2", 2), event("different-e2", 2)]
        )
        .is_err());
    assert_eq!(store.checkpoint(&lease, &stream()).unwrap(), Some(one));
}

#[tokio::test]
async fn native_session_stays_local_with_hosted_inference_and_unknown_kind_fails() {
    use crate::domain::{
        ExecutionLocation, RuntimeTarget, SessionCreate, SessionKind, SessionService, SessionStatus,
    };
    let (_dir, db, _store, _lease, _) = setup();
    let sessions = SessionService::new(db);
    for (id, target) in [
        (
            "native-cloud",
            RuntimeTarget::CloudRuntime {
                model: "hosted".into(),
                adapter: None,
            },
        ),
        (
            "native-remote",
            RuntimeTarget::RemoteRuntime {
                model: "hosted".into(),
                adapter: None,
                target_id: None,
            },
        ),
        (
            "native-local",
            RuntimeTarget::LocalRuntime {
                model: "local".into(),
                adapter: None,
            },
        ),
    ] {
        let mut session = sessions
            .create_or_update(SessionCreate {
                id: id.into(),
                title: "fixture".into(),
                kind: SessionKind::Codex,
                target,
                project_id: None,
                remote_id: None,
                codex_thread_id: None,
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap()
            .value;
        assert_eq!(
            session.execution_location().unwrap(),
            ExecutionLocation::Local
        );
        session.kind = "unknown".into();
        assert!(session.execution_location().is_err());
    }
}

#[test]
fn scoped_history_and_empty_swarm_wake_are_fenced() {
    let (_dir, _db, store, a, session) = setup();
    let checkpoint = Checkpoint::Intern {
        sequence: 1,
        generation: 0,
    };
    store
        .commit_page(&a, &stream(), None, &checkpoint, &[event("e1", 1)])
        .unwrap();
    assert_eq!(store.event_payloads(&a, &stream(), 10).unwrap().len(), 1);
    let swarm = Stream {
        adapter: Adapter::Swarm,
        external_id: "swarm".into(),
    };
    store.bind_session(&a, &swarm, &session).unwrap();
    let wake = Checkpoint::Swarm {
        event_id: Some("wake".into()),
        state_version: None,
        transcript_cursor: None,
    };
    assert!(store.commit_page(&a, &swarm, None, &wake, &[]).is_err());
    let b = store.activate_verified(&identity("b")).unwrap();
    store.create_conversation(&b, &stream(), "B").unwrap();
    assert!(store.event_payloads(&a, &stream(), 10).is_err());
    assert!(store.event_payloads(&b, &stream(), 10).unwrap().is_empty());
}

#[test]
fn restart_reconciles_only_nonterminal_remote_runs() {
    let (_dir, db, store, lease, _) = setup();
    for run in ["active", "done"] {
        store.bind_execution(&lease, &stream(), run).unwrap();
    }
    store
        .observe_execution(
            &lease,
            Adapter::InternSync,
            "active",
            RemoteExecutionState::Running,
        )
        .unwrap();
    store
        .observe_execution(
            &lease,
            Adapter::InternSync,
            "done",
            RemoteExecutionState::Completed,
        )
        .unwrap();
    assert!(store
        .observe_execution(
            &lease,
            Adapter::InternSync,
            "done",
            RemoteExecutionState::Running
        )
        .is_err());
    let reopened = CloudStore::open(db.clone()).unwrap();
    assert!(reopened
        .observe_execution(
            &lease,
            Adapter::InternSync,
            "active",
            RemoteExecutionState::Failed
        )
        .is_err());
    db.with_conn(|conn| {let states=conn.prepare("SELECT external_run_id,remote_state FROM cloud_execution_bindings ORDER BY external_run_id")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;assert_eq!(states,vec![("active".into(),"reconciling".into()),("done".into(),"completed".into())]);Ok(())}).unwrap();
}

#[test]
fn outbox_extends_shared_receipts_and_delivered_is_not_completed() {
    let (_dir, db, store, lease, _) = setup();
    store.enqueue(&lease, &intent()).unwrap();
    let local = local_command_id(&lease, "cmd-1").unwrap();
    let status = || {
        db.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT status FROM command_receipts WHERE command_id=?1",
                params![local],
                |r| r.get::<_, String>(0),
            )?)
        })
        .unwrap()
    };
    assert_eq!(status(), "accepted");
    store.begin_send(&lease, "cmd-1").unwrap();
    store
        .record_receipt(&lease, "cmd-1", ReceiptStage::Delivered, &json!({}))
        .unwrap();
    assert_eq!(status(), "accepted");
    store
        .record_receipt(&lease, "cmd-1", ReceiptStage::Applied, &json!({}))
        .unwrap();
    assert_eq!(status(), "completed");
}
