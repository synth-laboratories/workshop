use super::event_pump::{
    automatic_approval_response, normalized_turn_method, provider_child_env, rejection_response,
    safe_approval_payload, CREDENTIAL_ENV_NAMES,
};
use super::home::{
    apply_brokered_credential, apply_synth_cloud_provider, auto_compact_token_limit,
    automatic_thread_title, ensure_home, mcp_enabled_tools, multi_agent_flags, nested_id,
    normalize_gateway_origin, provider_class, requires_disabled_response_storage,
    responses_base_url, safe_component, supports_provider_compaction, toml_string,
    validate_reasoning_effort, validate_start, workspace_write_config, ProviderClass,
};
use super::manager::{reconcile_detached_status, CodexManager};
use super::proto::{
    is_detached_failure, select_approval_decision, CodexSessionRecord, CodexSessionStartRequest,
    CodexSteerRequest, CodexTurnFailure, CodexTurnSendRequest, CodexTurnStartRequest,
    ProviderTransport, SessionDetached, CODEX_SESSION_DETACHED, CODEX_TURN_START_FAILED,
    DETACHED_MESSAGE, MIN_AUTO_COMPACT_TOKEN_LIMIT, STDOUT_CLOSED,
};
use super::telemetry::{extract_turn_usage, finalize_performance_tracker, is_output_delta,
    settled_cost_from_receipts, PerformanceTrackers, TurnPerformanceTracker, TurnTokenUsage};
use crate::core_runtime::CoreRuntime;
use crate::credential_broker::{self, CredentialBroker};
use crate::domain::{
    RunCreate, RunService, RunStatus, RuntimeTarget, SessionCreate, SessionKind, SessionService,
    SessionStatus,
};
use crate::session::SessionPersistence;
use crate::storage::{CostSource, EventSource, MeasurementKind, UsageBreakdown, UsageRecord, UsageRecordsRepository};
use crate::synth_config::MultiAgentVersion;
use anyhow::anyhow;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tempfile::tempdir;

fn fixture_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex_app_server.py")
}

fn test_request(workspace: &Path, session_id: &str) -> CodexSessionStartRequest {
    CodexSessionStartRequest {
        session_id: session_id.into(),
        workspace: workspace.display().to_string(),
        base_url: "http://127.0.0.1:7333".into(),
        api_key: String::new(),
        model: "poolside/Laguna-XS-2.1-NVFP4-mlx".into(),
        provider_name: Some("local-laguna".into()),
        provider_title: Some("Laguna fixture".into()),
        provider_env_key: Some("SYNTH_LAGUNA_API_KEY".into()),
        approval_policy: Some("never".into()),
        sandbox: Some("workspace-write".into()),
        thread_id: None,
        multi_agent_version: Some(MultiAgentVersion::None),
        auto_compact_token_limit: None,
        writable_roots: Vec::new(),
        broker_credential: false,
    }
}

async fn wait_for_record_status(manager: &CodexManager, session_id: &str, expected: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let actual = manager
                .records
                .read()
                .await
                .get(session_id)
                .map(|record| record.status.clone());
            if actual.as_deref() == Some(expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("session {session_id} did not become {expected}"));
}

async fn wait_for_run_status(core: &CoreRuntime, run_id: &str, expected: &str) {
    let runs = RunService::new(core.storage().database().clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let actual = runs
                .get(run_id.to_owned())
                .await
                .unwrap()
                .map(|run| run.status);
            if actual.as_deref() == Some(expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("run {run_id} did not become {expected}"));
}

fn fixture_requests(root: &Path, session_id: &str) -> Vec<Value> {
    let path = root
        .join("homes")
        .join(safe_component(session_id))
        .join("fake-app-server-requests.jsonl");
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn killed_app_server_interrupts_sqlite_and_resumes_the_same_thread() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager =
        CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root.clone(), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let request = test_request(temp.path(), "crash-resume");

    let started = manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    assert_eq!(started.thread_id, "thread-fixture");
    let first_turn = manager
        .start_turn(
            app_handle.clone(),
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "keep working until the process is killed".into(),
                effort: Some("none".into()),
            },
        )
        .await
        .unwrap()
        .turn_id
        .unwrap();
    let first_attachment = manager
        .sessions
        .read()
        .await
        .get(&request.session_id)
        .unwrap()
        .clone();

    first_attachment.server.stop().await.unwrap();
    wait_for_record_status(&manager, &request.session_id, SessionStatus::Interrupted.as_str()).await;
    wait_for_run_status(&core, &first_turn, RunStatus::Interrupted.as_str()).await;
    assert!(!manager
        .sessions
        .read()
        .await
        .contains_key(&request.session_id));
    let persisted_run = RunService::new(core.storage().database().clone())
        .get(first_turn.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted_run.status, RunStatus::Interrupted.as_str());
    assert_eq!(
        persisted_run.outcome.unwrap()["reason"],
        "app_server_exited"
    );

    // Stop remains safe after the process and active attachment are gone.
    manager.interrupt(&request.session_id).await.unwrap();

    let resumed = manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    assert_eq!(resumed.thread_id, started.thread_id);
    let second_turn = manager
        .start_turn(
            app_handle,
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "continue after reconnect".into(),
                effort: Some("none".into()),
            },
        )
        .await
        .unwrap()
        .turn_id
        .unwrap();
    assert_ne!(first_turn, second_turn);
    let requests = fixture_requests(&codex_root, &request.session_id);
    assert!(requests.iter().any(|message| {
        message["method"] == "thread/resume"
            && message["params"]["threadId"] == "thread-fixture"
    }));
    manager.close(&request.session_id).await.unwrap();
}

#[tokio::test]
async fn steer_turn_sends_turn_steer_with_the_active_turn_id() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager =
        CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root.clone(), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let request = test_request(temp.path(), "steer-me");

    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let turn_id = manager
        .start_turn(
            app_handle.clone(),
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "keep working on the task".into(),
                effort: Some("none".into()),
            },
        )
        .await
        .unwrap()
        .turn_id
        .unwrap();

    manager
        .steer_turn(
            app_handle.clone(),
            CodexSteerRequest {
                session_id: request.session_id.clone(),
                text: "actually, focus on the tests first".into(),
            },
        )
        .await
        .unwrap();

    // The turn id is unchanged: steering augments the in-flight turn
    // rather than starting a new one.
    assert_eq!(
        manager
            .sessions
            .read()
            .await
            .get(&request.session_id)
            .unwrap()
            .turn_id
            .read()
            .await
            .clone(),
        Some(turn_id.clone())
    );

    let requests = fixture_requests(&codex_root, &request.session_id);
    let steer = requests
        .iter()
        .find(|message| message["method"] == "turn/steer")
        .expect("fixture did not see turn/steer");
    assert_eq!(steer["params"]["threadId"], "thread-fixture");
    assert_eq!(steer["params"]["expectedTurnId"], turn_id);
    assert_eq!(
        steer["params"]["input"][0]["text"],
        "actually, focus on the tests first"
    );
    assert_eq!(steer["params"]["input"][0]["type"], "text");

    manager.close(&request.session_id).await.unwrap();
}

#[tokio::test]
async fn compact_sends_thread_compact_start_for_the_attached_thread() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(SessionPersistence::from_core(Some(core)), codex_root.clone(), fixture_binary());
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "compact-me");

    manager
        .compact(app.handle().clone(), request.clone())
        .await
        .unwrap();

    let requests = fixture_requests(&codex_root, &request.session_id);
    let compact = requests
        .iter()
        .find(|message| message["method"] == "thread/compact/start")
        .expect("fixture did not see thread/compact/start");
    assert_eq!(compact["params"]["threadId"], "thread-fixture");

    manager.close(&request.session_id).await.unwrap();
}

#[tokio::test]
async fn steer_turn_fails_when_there_is_no_active_turn() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager =
        CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root.clone(), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let request = test_request(temp.path(), "steer-without-turn");

    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();

    let error = manager
        .steer_turn(
            app_handle.clone(),
            CodexSteerRequest {
                session_id: request.session_id.clone(),
                text: "hello".into(),
            },
        )
        .await
        .expect_err("steering without an active turn must fail");
    assert!(error.to_string().contains("no active turn"));

    manager.close(&request.session_id).await.unwrap();
}

#[tokio::test]
async fn startup_reconciles_an_orphaned_running_turn_in_sqlite() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    fs::create_dir_all(&codex_root).unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let sessions = SessionService::new(core.storage().database().clone());
    sessions
        .create_or_update(SessionCreate {
            id: "orphan".into(),
            title: "Orphaned local turn".into(),
            kind: SessionKind::Codex,
            target: RuntimeTarget::local_laguna(),
            project_id: None,
            remote_id: None,
            codex_thread_id: Some("thread-orphan".into()),
            status: SessionStatus::Ready,
            state_generation: None,
            metadata: json!({}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
    RunService::new(core.storage().database().clone())
        .start(RunCreate {
            id: "turn-orphan".into(),
            session_id: "orphan".into(),
            mode: "codex_turn".into(),
            model: Some("laguna".into()),
            adapter: None,
            metadata: json!({}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
    let record = CodexSessionRecord {
        session_id: "orphan".into(),
        thread_id: "thread-orphan".into(),
        workspace: temp.path().display().to_string(),
        model: "laguna".into(),
        provider_name: "local-laguna".into(),
        provider_title: "Laguna fixture".into(),
        base_url: "http://127.0.0.1:7333/v1".into(),
        status: SessionStatus::Running.as_str().into(),
        title: Some("Orphaned local turn".into()),
        title_origin: Some("automatic".into()),
        approval_policy: "never".into(),
        sandbox: "workspace-write".into(),
    };
    fs::write(
        codex_root.join("threads.json"),
        serde_json::to_vec_pretty(&HashMap::from([("orphan".to_owned(), record)])).unwrap(),
    )
    .unwrap();

    let restarted = CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root, fixture_binary());
    assert_eq!(restarted.list().await[0].status, SessionStatus::Interrupted.as_str());
    let run = RunService::new(core.storage().database().clone())
        .get("turn-orphan".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, RunStatus::Interrupted.as_str());
    assert_eq!(run.outcome.unwrap()["reason"], "desktop_restarted");
    assert_eq!(
        sessions.get("orphan".into()).await.unwrap().unwrap().status,
        SessionStatus::Interrupted.as_str()
    );
}

#[tokio::test]
async fn startup_reconciles_sqlite_when_detached_record_is_already_interrupted() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    fs::create_dir_all(&codex_root).unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let sessions = SessionService::new(core.storage().database().clone());
    sessions
        .create_or_update(SessionCreate {
            id: "orphan-after-graceful-exit".into(),
            title: "Partially reconciled local turn".into(),
            kind: SessionKind::Codex,
            target: RuntimeTarget::local_laguna(),
            project_id: None,
            remote_id: None,
            codex_thread_id: Some("thread-partial".into()),
            status: SessionStatus::Ready,
            state_generation: None,
            metadata: json!({}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
    RunService::new(core.storage().database().clone())
        .start(RunCreate {
            id: "turn-partial".into(),
            session_id: "orphan-after-graceful-exit".into(),
            mode: "codex_turn".into(),
            model: Some("laguna".into()),
            adapter: None,
            metadata: json!({}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
    let record = CodexSessionRecord {
        session_id: "orphan-after-graceful-exit".into(),
        thread_id: "thread-partial".into(),
        workspace: temp.path().display().to_string(),
        model: "laguna".into(),
        provider_name: "local-laguna".into(),
        provider_title: "Laguna fixture".into(),
        base_url: "http://127.0.0.1:7333/v1".into(),
        status: SessionStatus::Interrupted.as_str().into(),
        title: Some("Partially reconciled local turn".into()),
        title_origin: Some("automatic".into()),
        approval_policy: "never".into(),
        sandbox: "workspace-write".into(),
    };
    fs::write(
        codex_root.join("threads.json"),
        serde_json::to_vec_pretty(&HashMap::from([(
            "orphan-after-graceful-exit".to_owned(),
            record,
        )]))
        .unwrap(),
    )
    .unwrap();

    let restarted = CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root, fixture_binary());
    assert_eq!(restarted.list().await[0].status, SessionStatus::Interrupted.as_str());
    let run = RunService::new(core.storage().database().clone())
        .get("turn-partial".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, RunStatus::Interrupted.as_str());
    assert_eq!(run.outcome.unwrap()["reason"], "desktop_restarted");
    assert_eq!(
        sessions
            .get("orphan-after-graceful-exit".into())
            .await
            .unwrap()
            .unwrap()
            .status,
        SessionStatus::Interrupted.as_str()
    );
}

fn session_home(root: &Path, session_id: &str) -> PathBuf {
    root.join("homes").join(safe_component(session_id))
}

/// Makes the fixture app-server exit the moment `turn/start` arrives.
/// `once` clears the marker first so a retry can succeed.
fn arm_turn_start_exit(root: &Path, session_id: &str, mode: &str) {
    let home = session_home(root, session_id);
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("exit-on-turn-start"), mode).unwrap();
}

fn disarm_turn_start_exit(root: &Path, session_id: &str) {
    let marker = session_home(root, session_id).join("exit-on-turn-start");
    if marker.exists() {
        fs::remove_file(marker).unwrap();
    }
}

fn send_request(start: CodexSessionStartRequest, prompt: &str) -> CodexTurnSendRequest {
    CodexTurnSendRequest {
        start,
        prompt: prompt.into(),
        effort: Some("none".into()),
        compact_before_model_switch: false,
    }
}

/// The screenshot bug: the app-server exits between attach and turn/start.
/// The renderer must get a typed detachment, and durable state must already
/// be reconciled when it does. A later retry resumes the same thread.
#[tokio::test]
async fn turn_send_reports_detachment_and_reconciles_before_returning() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager =
        CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root.clone(), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let request = test_request(temp.path(), "turn-send-detached");

    // A healthy first turn establishes the thread and an active SQLite run.
    let first = manager
        .send_turn(
            app_handle.clone(),
            send_request(request.clone(), "start the acceptance turn"),
        )
        .await
        .unwrap();
    let first_turn = first.turn_id.clone().unwrap();
    assert_eq!(first.thread_id, "thread-fixture");
    assert_eq!(
        manager.records.read().await[&request.session_id].status,
        SessionStatus::Running.as_str()
    );

    arm_turn_start_exit(&codex_root, &request.session_id, "always");
    let failure = manager
        .send_turn(
            app_handle.clone(),
            send_request(request.clone(), "this turn can never start"),
        )
        .await
        .expect_err("a dead app-server must reject the turn");
    assert_eq!(failure.code, CODEX_SESSION_DETACHED);
    assert_eq!(failure.message, DETACHED_MESSAGE);
    assert_eq!(failure.session_id, request.session_id);
    // The raw session id belongs in debug detail, never in the message.
    assert!(!failure.message.contains(&request.session_id));

    // Reconciliation already happened, so the UI can never show Working.
    let status = manager.records.read().await[&request.session_id]
        .status
        .clone();
    assert_ne!(status, SessionStatus::Running.as_str());
    assert!(
        status == SessionStatus::Interrupted.as_str()
            || status == SessionStatus::Ready.as_str(),
        "unexpected reconciled status: {status}"
    );
    assert!(!manager
        .sessions
        .read()
        .await
        .contains_key(&request.session_id));
    let run = RunService::new(core.storage().database().clone())
        .get(first_turn.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, RunStatus::Interrupted.as_str());
    // Whichever of the exiting stdout task and this command wins the race,
    // the run is closed with a lost-process reason and stops being active.
    let reason = run.outcome.unwrap()["reason"].as_str().unwrap().to_owned();
    assert!(
        reason == "turn_start_detached" || reason == "app_server_exited",
        "unexpected interruption reason: {reason}"
    );
    assert_eq!(
        SessionService::new(core.storage().database().clone())
            .get(request.session_id.clone())
            .await
            .unwrap()
            .unwrap()
            .active_run_id,
        None
    );
    // Stop stays idempotent while nothing is attached.
    manager.interrupt(&request.session_id).await.unwrap();

    // Retry reattaches, resumes the same Codex thread and succeeds.
    disarm_turn_start_exit(&codex_root, &request.session_id);
    let retried = manager
        .send_turn(
            app_handle,
            send_request(request.clone(), "this turn can never start"),
        )
        .await
        .unwrap();
    assert_eq!(retried.thread_id, "thread-fixture");
    assert_ne!(retried.turn_id.clone().unwrap(), first_turn);
    assert_eq!(
        manager.records.read().await[&request.session_id].status,
        SessionStatus::Running.as_str()
    );
    let requests = fixture_requests(&codex_root, &request.session_id);
    assert!(requests.iter().any(|message| {
        message["method"] == "thread/resume"
            && message["params"]["threadId"] == "thread-fixture"
    }));
    manager.close(&request.session_id).await.unwrap();
}

/// A single exit is absorbed inside the command: the renderer sees one
/// successful send, never a transient error it has to model.
#[tokio::test]
async fn turn_send_retries_once_through_a_dying_app_server() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let manager = CodexManager::with_paths(SessionPersistence::Null, codex_root.clone(), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let request = test_request(temp.path(), "turn-send-retry");

    arm_turn_start_exit(&codex_root, &request.session_id, "once");
    let info = manager
        .send_turn(
            app_handle,
            send_request(request.clone(), "survive one process exit"),
        )
        .await
        .unwrap();
    assert!(info.turn_id.is_some());
    assert_eq!(
        manager.records.read().await[&request.session_id].status,
        SessionStatus::Running.as_str()
    );
    manager.close(&request.session_id).await.unwrap();
}

/// Restored state after a crash or relaunch: the JSON record claims
/// `running` but nothing is attached. Sending must reattach and resume.
#[tokio::test]
async fn turn_send_reattaches_a_restored_running_record_without_an_attachment() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    fs::create_dir_all(&codex_root).unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let record = CodexSessionRecord {
        session_id: "restored-running".into(),
        thread_id: "thread-restored".into(),
        workspace: temp.path().display().to_string(),
        model: "laguna".into(),
        provider_name: "local-laguna".into(),
        provider_title: "Laguna fixture".into(),
        base_url: "http://127.0.0.1:7333/v1".into(),
        status: SessionStatus::Running.as_str().into(),
        title: Some("Restored local turn".into()),
        title_origin: Some("automatic".into()),
        approval_policy: "never".into(),
        sandbox: "workspace-write".into(),
    };
    fs::write(
        codex_root.join("threads.json"),
        serde_json::to_vec_pretty(&HashMap::from([("restored-running".to_owned(), record)]))
            .unwrap(),
    )
    .unwrap();

    let manager =
        CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root.clone(), fixture_binary());
    assert!(!manager
        .sessions
        .read()
        .await
        .contains_key("restored-running"));
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "restored-running");
    let info = manager
        .send_turn(
            app.handle().clone(),
            send_request(request.clone(), "reconnect and continue"),
        )
        .await
        .unwrap();
    assert_eq!(info.thread_id, "thread-restored");
    assert!(info.turn_id.is_some());
    let requests = fixture_requests(&codex_root, "restored-running");
    assert!(requests.iter().any(|message| {
        message["method"] == "thread/resume"
            && message["params"]["threadId"] == "thread-restored"
    }));
    manager.close("restored-running").await.unwrap();
}

/// The partially reconciled shape: JSON already says `interrupted` while
/// SQLite still holds an active run. A rejected send must close that run.
#[tokio::test]
async fn turn_send_interrupts_an_active_run_when_the_record_is_already_interrupted() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    fs::create_dir_all(&codex_root).unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let sessions = SessionService::new(core.storage().database().clone());
    sessions
        .create_or_update(SessionCreate {
            id: "half-reconciled".into(),
            title: "Half reconciled".into(),
            kind: SessionKind::Codex,
            target: RuntimeTarget::local_laguna(),
            project_id: None,
            remote_id: None,
            codex_thread_id: Some("thread-half".into()),
            status: SessionStatus::Ready,
            state_generation: None,
            metadata: json!({}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
    RunService::new(core.storage().database().clone())
        .start(RunCreate {
            id: "turn-half".into(),
            session_id: "half-reconciled".into(),
            mode: "codex_turn".into(),
            model: Some("laguna".into()),
            adapter: None,
            metadata: json!({}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
    let record = CodexSessionRecord {
        session_id: "half-reconciled".into(),
        thread_id: "thread-half".into(),
        workspace: temp.path().display().to_string(),
        model: "laguna".into(),
        provider_name: "local-laguna".into(),
        provider_title: "Laguna fixture".into(),
        base_url: "http://127.0.0.1:7333/v1".into(),
        status: SessionStatus::Interrupted.as_str().into(),
        title: Some("Half reconciled".into()),
        title_origin: Some("automatic".into()),
        approval_policy: "never".into(),
        sandbox: "workspace-write".into(),
    };
    fs::write(
        codex_root.join("threads.json"),
        serde_json::to_vec_pretty(&HashMap::from([("half-reconciled".to_owned(), record)]))
            .unwrap(),
    )
    .unwrap();

    let manager =
        CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root.clone(), fixture_binary());
    arm_turn_start_exit(&codex_root, "half-reconciled", "always");
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "half-reconciled");
    let failure = manager
        .send_turn(
            app.handle().clone(),
            send_request(request, "try to continue"),
        )
        .await
        .expect_err("the fixture never answers turn/start");
    assert_eq!(failure.code, CODEX_SESSION_DETACHED);
    assert_ne!(
        manager.records.read().await["half-reconciled"].status,
        SessionStatus::Running.as_str()
    );
    let run = RunService::new(core.storage().database().clone())
        .get("turn-half".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, RunStatus::Interrupted.as_str());
    assert_eq!(run.outcome.unwrap()["reason"], "turn_start_detached");
    assert_eq!(
        sessions
            .get("half-reconciled".into())
            .await
            .unwrap()
            .unwrap()
            .active_run_id,
        None
    );
}

#[tokio::test]
async fn rejected_turn_send_arguments_never_mark_the_session_running() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let manager = CodexManager::with_paths(SessionPersistence::Null, codex_root, fixture_binary());
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "invalid-turn");
    let blank = manager
        .send_turn(app.handle().clone(), send_request(request.clone(), "   "))
        .await
        .expect_err("a blank prompt is rejected");
    assert_eq!(blank.code, CODEX_TURN_START_FAILED);
    let bad_effort = manager
        .send_turn(
            app.handle().clone(),
            CodexTurnSendRequest {
                start: request.clone(),
                prompt: "hello".into(),
                effort: Some("ultra".into()),
                compact_before_model_switch: false,
            },
        )
        .await
        .expect_err("an unsupported effort is rejected");
    assert_eq!(bad_effort.code, CODEX_TURN_START_FAILED);
    // Neither rejection may spawn an app-server or claim the session runs.
    assert!(manager.sessions.read().await.is_empty());
    assert!(manager
        .records
        .read()
        .await
        .get(&request.session_id)
        .is_none());
}

#[tokio::test]
async fn turn_send_compacts_on_source_model_before_rebind() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager =
        CodexManager::with_paths(SessionPersistence::from_core(Some(core.clone())), codex_root.clone(), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "compact-before-switch");

    let first = manager
        .send_turn(
            app_handle.clone(),
            send_request(request.clone(), "establish history on source model"),
        )
        .await
        .expect("first turn starts");
    assert!(first.turn_id.is_some());

    request.model = "openai/gpt-5.6-luna".into();
    request.provider_name = Some("openrouter".into());
    let switched = manager
        .send_turn(
            app_handle.clone(),
            CodexTurnSendRequest {
                start: request.clone(),
                prompt: "continue on destination".into(),
                effort: Some("medium".into()),
                compact_before_model_switch: true,
            },
        )
        .await
        .expect("switch turn starts after compact");
    assert_eq!(switched.thread_id, first.thread_id);
    assert!(switched.turn_id.is_some());

    let messages = fixture_requests(&codex_root, &request.session_id);
    let methods: Vec<&str> = messages
        .iter()
        .filter_map(|message| message.get("method").and_then(Value::as_str))
        .collect();
    assert!(
        methods
            .iter()
            .any(|method| *method == "thread/compact/start"),
        "expected compact before rebind, got {methods:?}"
    );
    let compact_idx = methods
        .iter()
        .position(|method| *method == "thread/compact/start")
        .unwrap();
    let turn_after_compact = methods
        .iter()
        .enumerate()
        .filter(|(_, method)| **method == "turn/start")
        .map(|(idx, _)| idx)
        .max()
        .unwrap();
    assert!(turn_after_compact > compact_idx);
    assert_eq!(
        manager
            .records
            .read()
            .await
            .get(&request.session_id)
            .map(|record| record.model.as_str()),
        Some("openai/gpt-5.6-luna")
    );
    assert!(
        manager
            .pending_compact_sources
            .lock()
            .await
            .get(&request.session_id)
            .is_none(),
        "model-switch compact source should be consumed when thread/compacted arrives"
    );
    let events = core
        .journal()
        .session_events_after(request.session_id.clone(), 0, 200)
        .await
        .expect("session events");
    assert!(
        events.iter().any(|event| {
            event.kind == "thread/compacted"
                && event.payload.get("source").and_then(Value::as_str) == Some("model_switch")
        }),
        "expected persisted thread/compacted with source=model_switch, got {:?}",
        events
            .iter()
            .map(|event| (&event.kind, event.payload.get("source")))
            .collect::<Vec<_>>()
    );
}

#[test]
fn only_lost_process_failures_are_treated_as_detachment() {
    assert!(is_detached_failure(
        &anyhow!(SessionDetached).context("Codex session abc")
    ));
    assert!(!is_detached_failure(&anyhow!(
        "codex app-server turn/start error: model unavailable"
    )));
}

#[tokio::test]
async fn stale_attachment_exit_cannot_detach_its_replacement() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let manager = CodexManager::with_paths(SessionPersistence::Null, codex_root, fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let request = test_request(temp.path(), "generation-fence");
    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let stale = manager
        .sessions
        .write()
        .await
        .remove(&request.session_id)
        .unwrap();
    manager.start(app_handle, request.clone()).await.unwrap();
    let replacement_id = manager
        .sessions
        .read()
        .await
        .get(&request.session_id)
        .unwrap()
        .attachment_id;
    assert_ne!(stale.attachment_id, replacement_id);

    stale.server.stop().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let current = manager
        .sessions
        .read()
        .await
        .get(&request.session_id)
        .unwrap()
        .attachment_id;
    assert_eq!(current, replacement_id);
    assert_eq!(
        manager.records.read().await[&request.session_id].status,
        "ready"
    );
    manager.close(&request.session_id).await.unwrap();
}

#[test]
fn extracts_flat_and_nested_ids() {
    assert_eq!(
        nested_id(&json!({"threadId":"a"}), "threadId").as_deref(),
        Some("a")
    );
    assert_eq!(
        nested_id(&json!({"thread":{"id":"b"}}), "threadId").as_deref(),
        Some("b")
    );
}
#[test]
fn approvals_always_fail_closed() {
    let denied = rejection_response(&["decline".into(), "acceptForSession".into()], json!(7));
    assert_eq!(
        denied.pointer("/result/decision").and_then(Value::as_str),
        Some("decline")
    );
    let rejected = rejection_response(&["accept".into(), "acceptForSession".into()], json!(8));
    assert_eq!(
        rejected.pointer("/error/code").and_then(Value::as_i64),
        Some(-32602)
    );
}
#[test]
fn allow_all_auto_approves_without_emitting_a_modal() {
    let session = automatic_approval_response(
        &["decline".into(), "accept".into(), "acceptForSession".into()],
        json!(9),
    );
    assert_eq!(
        session.pointer("/result/decision").and_then(Value::as_str),
        Some("acceptForSession")
    );
    let once = automatic_approval_response(&["accept".into()], json!(10));
    assert_eq!(
        once.pointer("/result/decision").and_then(Value::as_str),
        Some("accept")
    );
}
#[test]
fn approval_decisions_only_use_server_supported_values() {
    let available = vec!["decline".into(), "accept".into(), "acceptForSession".into()];
    assert_eq!(
        select_approval_decision(&available, "once").unwrap(),
        "accept"
    );
    assert_eq!(
        select_approval_decision(&available, "always").unwrap(),
        "acceptForSession"
    );
    assert_eq!(
        select_approval_decision(&available, "reject").unwrap(),
        "decline"
    );
    assert!(select_approval_decision(&available, "unknown").is_err());
    assert!(select_approval_decision(&["accept".into()], "reject").is_err());
}
#[test]
fn approval_payload_does_not_expose_command_or_arbitrary_reason() {
    let payload = safe_approval_payload(
        "approval-1",
        "item/commandExecution/requestApproval",
        &json!({
            "command":"OPENROUTER_API_KEY=secret curl example.test",
            "cwd":"/workspace",
            "reason":"raw model-supplied detail"
        }),
        &["decline".into(), "accept".into(), "acceptForSession".into()],
    );
    assert_eq!(payload["detail"], "Run a shell command in /workspace");
    assert_eq!(payload["alwaysSupported"], true);
    let encoded = payload.to_string();
    assert!(!encoded.contains("secret"));
    assert!(!encoded.contains("raw model"));
}
#[test]
fn sanitizes_session_home_component() {
    assert_eq!(safe_component("a/b c"), "a_b_c");
}
#[test]
fn escapes_toml_values() {
    assert_eq!(toml_string("a\"b\\c\n"), "a\\\"b\\\\c\\n");
}
#[test]
fn renders_additional_workspace_roots_for_codex() {
    assert_eq!(workspace_write_config(&[]), "");
    let config =
        workspace_write_config(&["/Users/example/Documents/GitHub".into(), "/tmp/a\"b".into()]);
    assert!(config.contains("[sandbox_workspace_write]"));
    assert!(config
        .contains("writable_roots = [\"/Users/example/Documents/GitHub\", \"/tmp/a\\\"b\"]"));
}
#[test]
fn advertises_only_the_compact_visual_tool_to_codex() {
    assert_eq!(
        mcp_enabled_tools("synth_visuals"),
        "enabled_tools = [\"visual_manage\"]\n"
    );
    assert_eq!(mcp_enabled_tools("synth_containers"), "");
    assert_eq!(
        mcp_enabled_tools("synth_optimizers"),
        "enabled_tools = [\"optimizer_manage\"]\n"
    );
}
#[test]
fn normalizes_responses_provider_base_url() {
    assert_eq!(
        responses_base_url("http://127.0.0.1:7333"),
        "http://127.0.0.1:7333/v1"
    );
    assert_eq!(
        responses_base_url("https://provider.test/v1/"),
        "https://provider.test/v1"
    );
    assert_eq!(
        responses_base_url("http://127.0.0.1:41209/api/v1"),
        "http://127.0.0.1:41209/api/v1"
    );
}

#[test]
fn normalizes_a_gateway_origin_that_already_carries_the_api_v1_suffix() {
    assert_eq!(
        normalize_gateway_origin("http://127.0.0.1:41124/api/v1"),
        "http://127.0.0.1:41124"
    );
    assert_eq!(
        normalize_gateway_origin("https://gateway.example/api/v1/responses"),
        "https://gateway.example"
    );
    assert_eq!(
        normalize_gateway_origin("http://0.0.0.0:41124"),
        "http://127.0.0.1:41124"
    );
    assert_eq!(
        normalize_gateway_origin("https://gateway.example/"),
        "https://gateway.example"
    );
}

#[test]
fn synth_cloud_provider_does_not_double_the_api_v1_path() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "synth-cloud-double-path");
    apply_synth_cloud_provider(
        &mut request,
        "http://127.0.0.1:41124/api/v1",
        Some("sk_dev_double_path"),
    )
    .unwrap();
    assert_eq!(request.base_url, "http://127.0.0.1:41124/api/v1");
}

#[test]
fn synth_cloud_provider_writes_expected_config() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let (broker, _listener) = CredentialBroker::bind().unwrap();
    let mut request = test_request(&workspace, "synth-cloud-config");
    apply_synth_cloud_provider(
        &mut request,
        "http://127.0.0.1:41209",
        Some("sk_dev_00000000000000000000000000000001"),
    )
    .unwrap();
    apply_brokered_credential(&mut request, &broker).unwrap();
    request.model = "openrouter/poolside/laguna-s-2.1".into();
    ensure_home(&home, &request).unwrap();
    let config = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(config.contains("model = \"openrouter/poolside/laguna-s-2.1\""));
    assert!(config.contains("model_provider = \"synth-cloud\""));
    assert!(config.contains("[model_providers.\"synth-cloud\"]"));
    // Codex is pointed at the native proxy, not at the backend directly.
    assert!(config.contains(&format!(
        "base_url = \"{}/api/v1\"",
        broker.origin()
    )));
    assert!(config.contains("wire_api = \"responses\""));
    assert!(config.contains(&format!(
        "env_key = \"{}\"",
        credential_broker::LEASE_ENV_KEY
    )));
    assert!(!config.contains("SYNTH_API_KEY"));
    assert_eq!(
        broker.upstream_for(&request.api_key).as_deref(),
        Some("http://127.0.0.1:41209")
    );
    assert!(config.contains("model_auto_compact_token_limit = 250000"));
    assert!(config.contains("tool_output_token_limit = 12000"));
    assert!(config.contains("CONTEXT CHECKPOINT COMPACTION for a coding agent"));
    // The Laguna gateway behind synth-cloud is a stateless passthrough
    // with no server-side session store: Codex must send `store: false`
    // and full history on every turn rather than leaning on
    // `previous_response_id` / `submit_tool_outputs` continuity the
    // gateway cannot serve.
    assert!(config.contains("disable_response_storage = true"));
    let optimizer_skill =
        fs::read_to_string(home.join("skills/use-synth-optimizers/SKILL.md")).unwrap();
    assert!(optimizer_skill.contains("optimizer_manage"));
}

#[test]
fn only_synth_cloud_gets_disable_response_storage() {
    let temp = tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let mut openrouter = test_request(&workspace, "openrouter-storage");
    openrouter.provider_name = Some("openrouter".into());
    openrouter.provider_title = Some("OpenRouter Responses".into());
    openrouter.base_url = "https://openrouter.ai/api/v1".into();
    let openrouter_home = temp.path().join("openrouter-home");
    ensure_home(&openrouter_home, &openrouter).unwrap();
    let openrouter_config = fs::read_to_string(openrouter_home.join("config.toml")).unwrap();
    assert!(!openrouter_config.contains("disable_response_storage"));

    let mut local_laguna = test_request(&workspace, "local-laguna-storage");
    local_laguna.provider_name = Some("local-laguna".into());
    local_laguna.provider_title = Some("Local Laguna".into());
    local_laguna.base_url = "http://127.0.0.1:7333".into();
    let local_laguna_home = temp.path().join("local-laguna-home");
    ensure_home(&local_laguna_home, &local_laguna).unwrap();
    let local_laguna_config =
        fs::read_to_string(local_laguna_home.join("config.toml")).unwrap();
    assert!(!local_laguna_config.contains("disable_response_storage"));

    assert!(requires_disabled_response_storage(&{
        let mut synth_cloud = test_request(&workspace, "synth-cloud-storage");
        synth_cloud.provider_name = Some("synth-cloud".into());
        synth_cloud
    }));
    assert!(!requires_disabled_response_storage(&openrouter));
    assert!(!requires_disabled_response_storage(&local_laguna));
}

#[test]
fn synth_cloud_provider_fails_closed_without_api_key() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "synth-cloud-missing-key");
    request.api_key = "renderer-supplied-should-not-matter".into();
    request.provider_name = Some("synth-cloud".into());
    let error = apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", None)
        .expect_err("missing Synth API key must fail closed");
    assert!(error.contains("Synth API key not configured"));
    assert!(error.contains("Settings → Account"));
    assert_eq!(request.api_key, "renderer-supplied-should-not-matter");
    assert!(!request.broker_credential);
}

#[test]
fn rejects_auto_compact_limits_outside_the_desktop_range() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "compact-limit");
    request.auto_compact_token_limit = Some(MIN_AUTO_COMPACT_TOKEN_LIMIT - 1);
    assert!(validate_start(&request)
        .unwrap_err()
        .to_string()
        .contains("autoCompactTokenLimit"));
    request.auto_compact_token_limit = Some(235_930);
    assert!(validate_start(&request)
        .unwrap_err()
        .to_string()
        .contains("autoCompactTokenLimit"));
}

#[test]
fn defaults_luna_and_laguna_s_compaction_to_250k() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "compact-defaults");
    request.model = "poolside/Laguna-XS-2.1-NVFP4-mlx".into();
    assert_eq!(auto_compact_token_limit(&request), 150_000);
    request.model = "poolside/laguna-s-2.1".into();
    assert_eq!(auto_compact_token_limit(&request), 250_000);
    request.model = "openai/gpt-5.6-luna".into();
    assert_eq!(auto_compact_token_limit(&request), 250_000);
}

#[test]
fn leaves_compaction_to_openai_and_azure_responses_providers() {
    let temp = tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let mut openai = test_request(&workspace, "openai-compaction");
    openai.provider_name = Some("openai".into());
    openai.provider_title = Some("OpenAI".into());
    openai.base_url = "https://api.openai.com/v1".into();
    openai.model = "gpt-5.6-luna".into();
    openai.auto_compact_token_limit = Some(999_999_999);
    validate_start(&openai).unwrap();
    let openai_home = temp.path().join("openai-home");
    ensure_home(&openai_home, &openai).unwrap();
    let openai_config = fs::read_to_string(openai_home.join("config.toml")).unwrap();
    assert!(!openai_config.contains("model_auto_compact_token_limit"));
    assert!(!openai_config.contains("compact_prompt"));

    let mut azure = test_request(&workspace, "azure-compaction");
    azure.provider_name = Some("custom-azure".into());
    azure.provider_title = Some("Azure".into());
    azure.base_url = "https://example.openai.azure.com/openai/v1".into();
    assert!(supports_provider_compaction(&azure));

    let mut openrouter = test_request(&workspace, "openrouter-compaction");
    openrouter.provider_name = Some("openrouter".into());
    openrouter.provider_title = Some("OpenRouter Responses".into());
    openrouter.base_url = "https://openrouter.ai/api/v1".into();
    openrouter.model = "openai/gpt-5.6-luna".into();
    assert!(!supports_provider_compaction(&openrouter));
}

#[test]
fn synth_cloud_provider_overwrites_renderer_api_key() {
    let temp = tempdir().unwrap();
    let (broker, _listener) = CredentialBroker::bind().unwrap();
    let mut request = test_request(temp.path(), "synth-cloud-overwrite");
    request.api_key = "renderer-leaked-key".into();
    request.base_url = "https://evil.example/v1".into();
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209/", Some("sk_dev_real_key"))
        .unwrap();
    // Staging discards the renderer's value and endpoint outright.
    assert_eq!(request.api_key, "sk_dev_real_key");
    assert!(request.broker_credential);
    apply_brokered_credential(&mut request, &broker).unwrap();
    // At spawn the staged key is replaced by a lease rather than being
    // carried on the request.
    assert_ne!(request.api_key, "renderer-leaked-key");
    assert_ne!(request.api_key, "sk_dev_real_key");
    assert!(request.api_key.starts_with("sdl_"));
    assert_eq!(request.base_url, format!("{}/api/v1", broker.origin()));
    assert_eq!(request.provider_name.as_deref(), Some("synth-cloud"));
    assert_eq!(
        request.provider_env_key.as_deref(),
        Some(credential_broker::LEASE_ENV_KEY)
    );
    assert_eq!(
        broker.upstream_for(&request.api_key).as_deref(),
        Some("http://127.0.0.1:41209")
    );
}

#[test]
fn synth_cloud_normalizes_a_local_bind_address_for_the_client() {
    let temp = tempdir().unwrap();
    let (broker, _listener) = CredentialBroker::bind().unwrap();
    let mut request = test_request(temp.path(), "synth-cloud-loopback");
    apply_synth_cloud_provider(
        &mut request,
        "http://0.0.0.0:41209/",
        Some("sk_dev_00000000000000000000000000000001"),
    )
    .unwrap();
    apply_brokered_credential(&mut request, &broker).unwrap();
    validate_start(&request).unwrap();
    assert_eq!(request.base_url, format!("{}/api/v1", broker.origin()));
    // `0.0.0.0` is a bind address, not a destination; the proxy's upstream
    // must still be rewritten to loopback.
    assert_eq!(
        broker.upstream_for(&request.api_key).as_deref(),
        Some("http://127.0.0.1:41209")
    );
}

/// The CUA-found 401: preparing a send re-ran provider setup for a live
/// session, minted a fresh lease, and thereby killed the token the reused
/// child was still presenting. Preparing the same binding again must leave
/// the live child's lease untouched.
#[tokio::test]
async fn reusing_a_live_child_leaves_its_lease_untouched() {
    let temp = tempdir().unwrap();
    let manager = CodexManager::with_paths(SessionPersistence::Null, temp.path().join("codex"), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "lease-live-reuse");
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_reuse"))
        .unwrap();

    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let broker = credential_broker::shared().unwrap();
    let token = broker
        .token_for("lease-live-reuse")
        .expect("spawning the child mints its lease");

    // Second send: same staged binding, live child gets reused.
    manager.start(app_handle, request.clone()).await.unwrap();
    assert_eq!(
        broker.token_for("lease-live-reuse").as_deref(),
        Some(token.as_str())
    );
    assert!(broker.resolves(&token));

    manager.close("lease-live-reuse").await.unwrap();
    assert!(!broker.resolves(&token));
}

/// Rebind (for example a model switch) closes the old child, which revokes
/// its lease. The replacement child must be spawned with a lease minted
/// *after* that revocation — leasing during preparation handed it a token
/// `close()` had already deleted.
#[tokio::test]
async fn rebinding_a_session_spawns_the_new_child_with_a_live_lease() {
    let temp = tempdir().unwrap();
    let manager = CodexManager::with_paths(SessionPersistence::Null, temp.path().join("codex"), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "lease-rebind");
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_rebind"))
        .unwrap();
    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let broker = credential_broker::shared().unwrap();
    let before = broker.token_for("lease-rebind").unwrap();

    let mut switched = request.clone();
    switched.model = "openrouter/poolside/laguna-s-2.1".into();
    manager.start(app_handle, switched).await.unwrap();
    let after = broker
        .token_for("lease-rebind")
        .expect("the respawned child must hold a lease that survived close()");
    assert_ne!(before, after);
    assert!(!broker.resolves(&before));
    assert!(broker.resolves(&after));
}

/// Provider identity is part of the reuse comparison in its own right.
/// `provider_name` is the sole input to `provider_class`, which gates the
/// settled-receipt drain — two providers sharing endpoint, credential and
/// model must still respawn (revoking, and discarding queued receipts)
/// when the name changes, or a finalize under the new name could drain
/// receipts born under the old one.
#[tokio::test]
async fn a_provider_name_change_alone_respawns_the_child() {
    let temp = tempdir().unwrap();
    let manager = CodexManager::with_paths(SessionPersistence::Null, temp.path().join("codex"), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "lease-provider-identity");
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_shared"))
        .unwrap();
    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let broker = credential_broker::shared().unwrap();
    let before = broker.token_for("lease-provider-identity").unwrap();
    credential_broker::push_settled_receipt(credential_broker::SettledReceipt {
        session_id: "lease-provider-identity".into(),
        provider_response_id: "resp-born-under-old-name".into(),
        model: None,
        prompt_tokens: None,
        completion_tokens: None,
        cached_tokens: None,
        reasoning_tokens: None,
        cost_usd: Some(0.25),
        completed_at_ms: 0,
    });

    // Same endpoint, credential, model, workspace, approval and sandbox —
    // only the provider name differs.
    let mut renamed = request.clone();
    renamed.provider_name = Some("openrouter".into());
    manager.start(app_handle, renamed).await.unwrap();
    let after = broker
        .token_for("lease-provider-identity")
        .expect("the respawned child must hold a fresh lease");
    assert_ne!(before, after);
    assert!(!broker.resolves(&before));
    assert!(broker.resolves(&after));
    assert!(
        credential_broker::drain_settled_receipts("lease-provider-identity").is_empty(),
        "receipts born under the old provider name must not survive the switch"
    );
}

/// A changed credential or endpoint is part of the reuse comparison: the
/// old child was spawned against the old binding, so rotation must respawn
/// it rather than leave it talking through the stale credential.
#[tokio::test]
async fn a_rotated_credential_respawns_the_child_with_a_fresh_lease() {
    let temp = tempdir().unwrap();
    let manager = CodexManager::with_paths(SessionPersistence::Null, temp.path().join("codex"), fixture_binary());
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "lease-rotation");
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_old"))
        .unwrap();
    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let broker = credential_broker::shared().unwrap();
    let old_token = broker.token_for("lease-rotation").unwrap();
    let old_attachment = manager
        .sessions
        .read()
        .await
        .get("lease-rotation")
        .unwrap()
        .attachment_id;

    let mut rotated = test_request(temp.path(), "lease-rotation");
    apply_synth_cloud_provider(&mut rotated, "http://127.0.0.1:41209", Some("sk_dev_new"))
        .unwrap();
    manager.start(app_handle, rotated).await.unwrap();
    let new_attachment = manager
        .sessions
        .read()
        .await
        .get("lease-rotation")
        .unwrap()
        .attachment_id;
    let new_token = broker.token_for("lease-rotation").unwrap();
    assert_ne!(old_attachment, new_attachment);
    assert_ne!(old_token, new_token);
    assert!(!broker.resolves(&old_token));
    assert!(broker.resolves(&new_token));
}

#[test]
fn completed_envelope_with_a_failed_turn_is_normalized_to_failed() {
    let failed = json!({
        "turn": {
            "status": "failed",
            "error": {"message": "provider disconnected"}
        }
    });
    assert_eq!(
        normalized_turn_method("turn/completed", &failed),
        "turn/failed"
    );
    assert_eq!(
        normalized_turn_method("turn/completed", &json!({"turn": {"status": "completed"}})),
        "turn/completed"
    );
}

#[test]
fn invalid_provider_endpoint_explains_the_fix_without_leaking_credentials() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "invalid-provider-endpoint");
    request.provider_title = Some("Synth Cloud Responses".into());
    request.base_url =
        "http://user:secret-token@0.0.0.0:41209/api/v1?api_key=secret-token".into();

    let error = validate_start(&request).unwrap_err().to_string();

    assert!(error.contains("Synth Cloud Responses could not start"));
    assert!(error.contains("http://[credentials]@0.0.0.0:41209/api/v1"));
    assert!(error.contains("Settings → Account → Backend API"));
    assert!(!error.contains("secret-token"));
}

#[test]
fn synth_cloud_home_redacts_api_key_from_generated_files() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let secret = "sk_dev_SYNTH_CLOUD_SECRET_VALUE_DO_NOT_LEAK";
    let (broker, _listener) = CredentialBroker::bind().unwrap();
    let mut request = test_request(&workspace, "synth-cloud-redact");
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some(secret)).unwrap();
    apply_brokered_credential(&mut request, &broker).unwrap();
    request.model = "openrouter/poolside/laguna-s-2.1".into();
    ensure_home(&home, &request).unwrap();
    let config = fs::read_to_string(home.join("config.toml")).unwrap();
    let auth = fs::read_to_string(home.join("auth.json")).unwrap();
    assert!(!config.contains(secret));
    assert!(!auth.contains(secret));
    assert!(config.contains(&format!(
        "env_key = \"{}\"",
        credential_broker::LEASE_ENV_KEY
    )));
    assert!(auth.contains("synth-desktop-provider"));
}

/// Every path that ever wrote a file under a generated Codex home, checked
/// against one sentinel value: the credential must exist only in the native
/// broker.
///
/// `shell_snapshots` is the leak this guards. Codex serializes its inherited
/// environment there as `export NAME=value`, so the test reproduces that
/// step from the exact environment `spawn_server` would hand the child.
#[test]
fn the_synth_credential_never_reaches_a_generated_codex_home() {
    const SENTINEL: &str = "sk_live_SENTINEL_ONLY_IN_NATIVE_CUSTODY";
    let temp = tempdir().unwrap();
    let root = temp.path().join("codex");
    let home = root.join("homes/session-sentinel");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let (broker, _listener) = CredentialBroker::bind().unwrap();
    let mut request = test_request(&workspace, "session-sentinel");
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some(SENTINEL)).unwrap();
    apply_brokered_credential(&mut request, &broker).unwrap();
    validate_start(&request).unwrap();
    ensure_home(&home, &request).unwrap();

    // Stand in for the Codex child: write the snapshot it would write from
    // the environment we actually pass it.
    let snapshots = home.join("shell_snapshots");
    fs::create_dir_all(&snapshots).unwrap();
    let exported = provider_child_env(&request)
        .expect("the brokered lease is allowed across the spawn boundary")
        .map(|(name, value)| format!("export {name}={value}\n"))
        .unwrap_or_default();
    fs::write(snapshots.join("snapshot.sh"), format!("#!/bin/sh\n{exported}")).unwrap();
    // Session logs and event payloads are the other things a home accumulates.
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::write(
        home.join("sessions/rollout.jsonl"),
        serde_json::to_string(&json!({
            "base_url": request.base_url,
            "provider": request.provider_name,
            "env_key": request.provider_env_key,
        }))
        .unwrap(),
    )
    .unwrap();

    let mut scanned = 0usize;
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            scanned += 1;
            let bytes = fs::read(&path).unwrap();
            assert!(
                !String::from_utf8_lossy(&bytes).contains(SENTINEL),
                "the Synth credential reached {}",
                path.display()
            );
        }
    }
    assert!(scanned > 3, "the sentinel scan must actually read files");

    // The broker holds it, and only the broker.
    assert!(request.api_key.starts_with("sdl_"));
    assert!(broker.upstream_for(&request.api_key).is_some());
    // Nothing that renders to the user or a log can reproduce it either.
    let rendered = format!(
        "{:?} {:?} {}",
        broker,
        request.provider_env_key,
        validate_start(&request)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
    );
    assert!(!rendered.contains(SENTINEL));
}

#[test]
fn a_real_credential_variable_is_refused_at_the_spawn_boundary() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "spawn-boundary");
    request.api_key = "sk_live_should_never_be_exported".into();
    for name in CREDENTIAL_ENV_NAMES {
        request.provider_env_key = Some((*name).into());
        let error = provider_child_env(&request)
            .expect_err(&format!("{name} must never be exported to a Codex child"))
            .to_string();
        // The refusal names the variable and the way out of it.
        assert!(error.contains(name), "{error}");
        assert!(error.contains("credential broker"), "{error}");
        assert!(
            !error.contains("sk_live_should_never_be_exported"),
            "the refusal must not quote the credential: {error}"
        );
    }
    // The broker lease, and the local loopback token, still cross.
    request.provider_env_key = Some(credential_broker::LEASE_ENV_KEY.into());
    assert_eq!(
        provider_child_env(&request).unwrap(),
        Some((
            credential_broker::LEASE_ENV_KEY.to_owned(),
            "sk_live_should_never_be_exported".to_owned()
        ))
    );
    request.provider_env_key = None;
    request.api_key = String::new();
    assert_eq!(provider_child_env(&request).unwrap(), None);
}

#[test]
fn maps_model_capability_to_app_server_feature_flags() {
    assert_eq!(
        multi_agent_flags(MultiAgentVersion::None),
        (false, false, false)
    );
    assert_eq!(
        multi_agent_flags(MultiAgentVersion::V1),
        (true, true, false)
    );
    assert_eq!(multi_agent_flags(MultiAgentVersion::V2), (true, true, true));
}

#[test]
fn validates_reasoning_effort_values() {
    for value in ["none", "low", "medium", "high", "xhigh", "max"] {
        assert_eq!(validate_reasoning_effort(value).unwrap(), value);
    }
    assert!(validate_reasoning_effort("ultra").is_err());
}

#[test]
fn derives_a_short_title_from_the_first_prompt() {
    assert_eq!(
        automatic_thread_title(
            "please add session descriptions to the Rust core. Then test it"
        ),
        Some("Add session descriptions to the Rust core".into())
    );
    assert_eq!(
        automatic_thread_title(
            "Use $use-synth-containers to inspect Craftax and locate its real policy harness"
        ),
        Some("Inspect Craftax and locate its real policy harness".into())
    );
}

#[test]
fn detached_running_sessions_are_reconciled_as_interrupted() {
    let mut running = SessionStatus::Running.as_str().to_owned();
    assert!(reconcile_detached_status(&mut running, false));
    assert_eq!(running, SessionStatus::Interrupted.as_str());

    let mut attached = SessionStatus::Running.as_str().to_owned();
    assert!(!reconcile_detached_status(&mut attached, true));
    assert_eq!(attached, SessionStatus::Running.as_str());

    let mut ready = SessionStatus::Ready.as_str().to_owned();
    assert!(!reconcile_detached_status(&mut ready, false));
    assert_eq!(ready, SessionStatus::Ready.as_str());
}

#[test]
fn extracts_only_authoritative_per_turn_usage_shapes() {
    let snake_case = extract_turn_usage(&json!({
        "turn": {"usage": {
            "input_tokens": 100,
            "output_tokens": 25,
            "output_tokens_details": {"reasoning_tokens": 4}
        }}
    }))
    .unwrap();
    assert_eq!(snake_case.input_tokens, Some(100));
    assert_eq!(snake_case.output_tokens, Some(25));
    assert_eq!(snake_case.reasoning_tokens, Some(4));

    let camel_case = extract_turn_usage(&json!({
        "tokenUsage": {
            "totalUsage": {"inputTokens": 9999, "outputTokens": 9999},
            "lastUsage": {"inputTokens": 50, "outputTokens": 8, "cachedInputTokens": 20}
        }
    }))
    .unwrap();
    assert_eq!(camel_case.input_tokens, Some(50));
    assert_eq!(camel_case.output_tokens, Some(8));
    assert_eq!(camel_case.cached_input_tokens, Some(20));
    assert!(extract_turn_usage(&json!({
        "tokenUsage": {"totalUsage": {"inputTokens": 9999, "outputTokens": 9999}}
    }))
    .is_none());
}

#[test]
fn recognizes_answer_deltas_but_not_reasoning_or_empty_events() {
    assert!(is_output_delta(
        "item/agentMessage/delta",
        &json!({"delta": "answer"})
    ));
    assert!(!is_output_delta(
        "item/reasoning/delta",
        &json!({"delta": "private reasoning"})
    ));
    assert!(!is_output_delta(
        "item/agentMessage/delta",
        &json!({"delta": ""})
    ));
}

// ---- settled Synth Cloud accounting at turn finalize ----
// Session ids are unique per test: the broker's receipt store is
// process-wide and these tests run in parallel.

fn tracker_for(provider: &str, turn_id: &str) -> TurnPerformanceTracker {
    TurnPerformanceTracker {
        provider: provider.into(),
        model_id: "openrouter/poolside/laguna-s-2.1".into(),
        turn_id: turn_id.into(),
        started_at_ms: 1_000,
        first_output_at_ms: Some(1_100),
        last_output_at_ms: Some(1_900),
        usage: TurnTokenUsage {
            input_tokens: Some(1_000),
            cached_input_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            output_tokens: Some(200),
        },
    }
}

fn settled_receipt(
    session_id: &str,
    response_id: &str,
    cost_usd: Option<f64>,
) -> credential_broker::SettledReceipt {
    credential_broker::SettledReceipt {
        session_id: session_id.into(),
        provider_response_id: response_id.into(),
        model: Some("openrouter/poolside/laguna-s-2.1".into()),
        prompt_tokens: Some(500),
        completion_tokens: Some(100),
        cached_tokens: None,
        reasoning_tokens: None,
        cost_usd,
        completed_at_ms: 1_950,
    }
}

async fn finalize_turn(core: &Arc<CoreRuntime>, session_id: &str, provider: &str, turn: &str) {
    let trackers: PerformanceTrackers = Arc::default();
    trackers
        .lock()
        .await
        .insert(session_id.to_owned(), tracker_for(provider, turn));
    finalize_performance_tracker(&SessionPersistence::from_core(Some(core.clone())), &trackers, session_id, "completed", Some(2_000)).await;
}

async fn usage_totals(core: &Arc<CoreRuntime>) -> UsageBreakdown {
    UsageRecordsRepository::new(core.storage().database().clone())
        .summary("all".into(), None)
        .await
        .unwrap()
        .totals
}

#[test]
fn settled_cost_sums_only_receipts_that_carried_money() {
    assert_eq!(settled_cost_from_receipts(&[]), None);
    assert_eq!(
        settled_cost_from_receipts(&[settled_receipt("s", "a", None)]),
        None
    );
    let mixed = [
        settled_receipt("s", "a", Some(0.01)),
        settled_receipt("s", "b", None),
        settled_receipt("s", "c", Some(0.02)),
    ];
    let sum = settled_cost_from_receipts(&mixed).unwrap();
    assert!((sum - 0.03).abs() < 1e-12, "{sum}");
}

#[tokio::test]
async fn a_synth_cloud_turn_records_the_sum_of_its_settled_receipts() {
    let temp = tempdir().unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let session = "wp4-cloud-settles";
    // One turn may make several upstream requests; their settled charges
    // sum, and a token-only receipt contributes no invented money.
    credential_broker::push_settled_receipt(settled_receipt(session, "resp-1", Some(0.01)));
    credential_broker::push_settled_receipt(settled_receipt(session, "resp-2", Some(0.02)));
    credential_broker::push_settled_receipt(settled_receipt(session, "resp-3", None));
    finalize_turn(&core, session, "synth-cloud", "turn-1").await;

    let totals = usage_totals(&core).await;
    assert_eq!(totals.requests, 1);
    assert!((totals.billed_cost_usd.unwrap() - 0.03).abs() < 1e-12);
    assert_eq!(totals.cost_source, CostSource::SynthCloud);
    // Tokens stay the tracker's own counts, and the settled charge left
    // nothing in the estimate column.
    assert_eq!(totals.input_tokens, 1_000);
    assert_eq!(totals.output_tokens, 200);
    assert_eq!(totals.estimated_cost_usd, None);
    // Drained: a replayed finalize finds nothing to double-bill.
    assert!(credential_broker::drain_settled_receipts(session).is_empty());
}

#[tokio::test]
async fn cloud_receipts_without_money_leave_billed_unset() {
    let temp = tempdir().unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let session = "wp4-cloud-token-only";
    credential_broker::push_settled_receipt(settled_receipt(session, "resp-1", None));
    finalize_turn(&core, session, "synth-cloud", "turn-1").await;

    let totals = usage_totals(&core).await;
    assert_eq!(totals.requests, 1);
    assert_eq!(totals.billed_cost_usd, None);
    assert_eq!(totals.cost_source, CostSource::None);
    assert_eq!(totals.input_tokens, 1_000);
}

#[tokio::test]
async fn local_turns_neither_drain_receipts_nor_carry_any_charge() {
    let temp = tempdir().unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let session = "wp4-local-untouched";
    // Even a stray receipt under a local session's id must not turn an
    // on-device row into a billed one — billed stays None, never $0.
    credential_broker::push_settled_receipt(settled_receipt(session, "resp-1", Some(0.42)));
    finalize_turn(&core, session, "local-laguna", "turn-1").await;

    let totals = usage_totals(&core).await;
    assert_eq!(totals.requests, 1);
    assert_eq!(totals.billed_cost_usd, None);
    assert_eq!(totals.estimated_cost_usd, None);
    assert_eq!(totals.cost_source, CostSource::None);
    // The local finalize did not consume the queue.
    assert_eq!(credential_broker::drain_settled_receipts(session).len(), 1);
}

/// The cancellation-race contract: a receipt landing after its turn
/// finalized stays queued no longer than the session's next finalize, and
/// never becomes a row of its own.
#[tokio::test]
async fn a_late_receipt_waits_for_the_next_finalize_and_never_invents_a_row() {
    let temp = tempdir().unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let session = "wp4-late-receipt";
    finalize_turn(&core, session, "synth-cloud", "turn-1").await;
    let totals = usage_totals(&core).await;
    assert_eq!((totals.requests, totals.billed_cost_usd), (1, None));

    credential_broker::push_settled_receipt(settled_receipt(session, "resp-late", Some(0.05)));
    // Still exactly one row: a queued receipt is not a usage record.
    assert_eq!(usage_totals(&core).await.requests, 1);

    finalize_turn(&core, session, "synth-cloud", "turn-2").await;
    let totals = usage_totals(&core).await;
    assert_eq!(totals.requests, 2);
    assert!((totals.billed_cost_usd.unwrap() - 0.05).abs() < 1e-12);
    assert_eq!(totals.cost_source, CostSource::SynthCloud);
}