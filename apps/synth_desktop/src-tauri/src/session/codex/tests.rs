use super::event_pump::{
    approval_decisions, automatic_approval_decision, automatic_approval_response,
    is_approval_method, normalized_turn_method, provider_child_env, rejection_response,
    safe_approval_payload, CREDENTIAL_ENV_NAMES,
};
use super::home::{
    apply_brokered_credential, apply_local_laguna_catalog_metadata, apply_local_laguna_provider,
    apply_openrouter_provider, apply_synth_cloud_provider, auto_compact_token_limit,
    automatic_thread_title, credential_read_policy_comment, ensure_home,
    install_local_laguna_catalog, local_laguna_catalog, mcp_enabled_tools, mcp_env_config,
    mcp_ipc_env_key, multi_agent_flags, nested_id, normalize_gateway_origin, provider_class,
    requires_disabled_response_storage, responses_base_url, safe_component,
    supports_provider_compaction, toml_string, validate_reasoning_effort, validate_start,
    workspace_write_config, ProviderClass, OPENROUTER_RESPONSES_BASE_URL,
};

use super::generation_speed::{
    ols_tokens_per_second, protocol_event, ProtocolEvent, QualityFlag, SegmentPhase, SegmentStatus,
    TokenCountSource, TurnSegmentTracker, UnavailableReason,
};
use super::manager::CodexManager;
use super::proto::{
    is_detached_failure, select_approval_decision, CodexApprovalDecisionRequest,
    CodexSessionRecord, CodexSessionStartRequest, CodexSteerRequest, CodexTurnFailure,
    CodexTurnSendRequest, CodexTurnStartRequest, ProviderTransport, SessionDetached,
    CODEX_SESSION_DETACHED, CODEX_TURN_START_FAILED, DETACHED_MESSAGE,
    MIN_AUTO_COMPACT_TOKEN_LIMIT, STDOUT_CLOSED,
};
use super::telemetry::{
    extract_turn_usage, finalize_performance_tracker, is_output_delta, settled_cost_from_receipts,
    track_performance_event, PerformanceTrackers, TurnPerformanceTracker, TurnTokenUsage,
};
use crate::core_runtime::CoreRuntime;
use crate::credential_broker::{self, CredentialBroker};
use crate::domain::{
    RunCreate, RunService, RunStatus, RuntimeTarget, SessionCreate, SessionKind, SessionService,
    SessionStatus,
};
use crate::session::SessionPersistence;
use crate::storage::{
    CostSource, EventSource, GenerationSpeedRepository, MeasurementKind, UsageBreakdown,
    UsageRecord, UsageRecordsRepository,
};
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

fn model_catalog_entry(slug: &str, instructions: &str) -> Value {
    serde_json::json!({
        "slug": slug,
        "display_name": slug,
        "description": format!("metadata for {slug}"),
        "base_instructions": instructions,
        "default_reasoning_level": "high",
        "supported_reasoning_levels": [
            {"effort": "none", "description": "no reasoning"},
            {"effort": "high", "description": "reasoning"}
        ],
        "shell_type": "unified_exec",
        "supported_in_api": true,
        "service_tiers": [],
        "default_service_tier": "default",
        "context_window": 262144,
        "max_context_window": 262144,
        "input_modalities": ["text"],
        "supports_parallel_tool_calls": true,
        "supports_search_tool": false
    })
}

fn local_catalog_envelope() -> Value {
    serde_json::json!({"models": [model_catalog_entry(
        "poolside/Laguna-XS-2.1-NVFP4-mlx",
        "Laguna-owned local instructions."
    )]})
}

#[test]
fn local_laguna_catalog_selects_exact_model_as_a_unit_without_first_entry_leakage() {
    let mut unrelated = model_catalog_entry("openai/gpt-first", "unrelated bundled instructions");
    unrelated["service_tiers"] = serde_json::json!([{"id": "priority", "name": "Fast"}]);
    unrelated["default_service_tier"] = serde_json::json!("priority");
    unrelated["input_modalities"] = serde_json::json!(["text", "image"]);
    unrelated["supports_search_tool"] = serde_json::json!(true);
    let selected = model_catalog_entry(
        "poolside/Laguna-XS-2.1-NVFP4-mlx",
        "Laguna-owned local instructions.",
    );
    let catalog = local_laguna_catalog(
        serde_json::json!({"models": [unrelated, selected.clone()]}),
        "poolside/Laguna-XS-2.1-NVFP4-mlx",
    )
    .unwrap();

    assert_eq!(catalog["models"][0], selected);
    assert_eq!(catalog["models"][0]["service_tiers"], serde_json::json!([]));
    assert_eq!(
        catalog["models"][0]["input_modalities"],
        serde_json::json!(["text"])
    );
    assert_eq!(catalog["models"][0]["supports_search_tool"], false);
}

#[test]
fn local_laguna_provider_uses_daemon_identity_and_never_advertises_fast_tier() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "local-provider-binding");
    request.model = "laguna-xs-2.1".into();
    request.service_tier = Some("fast".into());

    apply_local_laguna_provider(&mut request, "poolside/Laguna-XS-2.1-NVFP4-mlx");

    assert_eq!(request.model, "poolside/Laguna-XS-2.1-NVFP4-mlx");
    assert_eq!(request.service_tier, None);
    apply_local_laguna_catalog_metadata(&mut request, local_catalog_envelope()).unwrap();
    assert_eq!(request.service_tier.as_deref(), Some("default"));
}

#[test]
fn local_laguna_catalog_rejects_missing_duplicate_or_incomplete_exact_entries() {
    let exact = model_catalog_entry(
        "poolside/Laguna-XS-2.1-NVFP4-mlx",
        "Laguna-owned local instructions.",
    );
    let mut incomplete = exact.clone();
    incomplete
        .as_object_mut()
        .unwrap()
        .remove("base_instructions");
    for envelope in [
        serde_json::json!({"models": []}),
        serde_json::json!({"models": [model_catalog_entry("other", "other")]}),
        serde_json::json!({"models": [exact.clone(), exact.clone()]}),
        serde_json::json!({"models": [incomplete]}),
    ] {
        assert!(local_laguna_catalog(envelope, "poolside/Laguna-XS-2.1-NVFP4-mlx").is_err());
    }
}

#[test]
fn local_laguna_catalog_installation_fails_closed_without_authenticated_metadata() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "catalog-fail-closed");
    request.local_model_catalog = None;
    let error = install_local_laguna_catalog(&temp.path().join("home"), &request)
        .expect_err("local provider must not silently omit its validated catalog");
    assert!(error
        .to_string()
        .contains("authenticated Laguna native Codex model catalog was not prepared"));
}

#[test]
fn local_laguna_catalog_installation_materializes_exact_daemon_metadata() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let request = test_request(temp.path(), "catalog-materialized");
    assert!(install_local_laguna_catalog(&home, &request).unwrap());
    let catalog: Value =
        serde_json::from_slice(&fs::read(home.join("model-catalog.json")).unwrap()).unwrap();
    assert_eq!(catalog["models"][0]["slug"], request.model);
    assert_eq!(
        catalog["models"][0]["base_instructions"],
        "Laguna-owned local instructions."
    );
}

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
        service_tier: None,
        thread_id: None,
        multi_agent_version: Some(MultiAgentVersion::None),
        auto_compact_token_limit: None,
        writable_roots: Vec::new(),
        local_model_catalog: Some(local_catalog_envelope()),
        broker_credential: false,
    }
}

#[tokio::test]
async fn missing_rollout_on_resume_starts_a_replacement_thread() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let mut request = test_request(temp.path(), "stale-thread");
    request.thread_id = Some("missing-rollout".into());
    let home = codex_root.join("homes").join("stale-thread");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("reject-thread-resume"), "1").unwrap();

    let info = manager.start(app.handle().clone(), request).await.unwrap();
    assert_eq!(info.thread_id, "thread-fixture");
    let requests = fixture_requests(&codex_root, "stale-thread");
    let methods: Vec<_> = requests
        .iter()
        .filter_map(|message| message["method"].as_str())
        .filter(|method| *method == "thread/resume" || *method == "thread/start")
        .collect();
    assert_eq!(methods, vec!["thread/resume", "thread/start"]);
}

/// These waits poll a spawned fixture process, so they are load-sensitive:
/// five seconds passed on an idle machine and failed intermittently while a
/// build or another suite ran alongside. The assertion is that the state
/// settles at all, not that it settles quickly — give it room so a busy CI box
/// does not read as a product defect.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);

async fn wait_for_record_status(manager: &CodexManager, session_id: &str, expected: &str) {
    tokio::time::timeout(SETTLE_TIMEOUT, async {
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
    tokio::time::timeout(SETTLE_TIMEOUT, async {
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

async fn wait_for_pending_approval(manager: &CodexManager) {
    tokio::time::timeout(SETTLE_TIMEOUT, async {
        loop {
            if manager.approvals.pending_len().await == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fixture approval did not reach the broker");
}

#[tokio::test]
async fn shell_approval_resolves_through_the_broker_and_drains_pending_state() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("untrusted", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "broker-resolve");
    request.approval_policy = Some("untrusted".into());
    let home = codex_root.join("homes").join("broker-resolve");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("request-approval-on-turn-start"), "1").unwrap();

    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    manager
        .start_turn(
            app_handle.clone(),
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "request fixture approval".into(),
                effort: Some("none".into()),
                client_message_id: None,
            },
        )
        .await
        .unwrap();
    wait_for_pending_approval(&manager).await;
    let events = core
        .journal()
        .session_events_after(request.session_id.clone(), 0, 100)
        .await
        .unwrap();
    let approval_id = events
        .iter()
        .find(|event| event.kind == "approval.requested")
        .and_then(|event| event.payload["approvalId"].as_str())
        .unwrap()
        .to_owned();

    manager
        .resolve_approval(
            app_handle,
            CodexApprovalDecisionRequest {
                session_id: request.session_id.clone(),
                approval_id: approval_id.clone(),
                decision: "once".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(manager.approvals.pending_len().await, 0);
    let requests = fixture_requests(&codex_root, &request.session_id);
    assert!(requests
        .iter()
        .any(|message| { message["id"] == 9001 && message["result"]["decision"] == "accept" }));
    let events = core
        .journal()
        .session_events_after(request.session_id.clone(), 0, 100)
        .await
        .unwrap();
    assert!(events.iter().any(|event| {
        event.kind == "approval.granted"
            && event.payload["approvalId"] == approval_id
            && event.payload["appServerDecision"] == "accept"
    }));
    manager.close(&request.session_id).await.unwrap();
}

#[tokio::test]
async fn dead_approval_origin_expires_and_drains_pending_state() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("untrusted", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let mut request = test_request(temp.path(), "broker-expire");
    request.approval_policy = Some("untrusted".into());
    let home = codex_root.join("homes").join("broker-expire");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("request-approval-on-turn-start"), "1").unwrap();

    manager
        .start(app.handle().clone(), request.clone())
        .await
        .unwrap();
    manager
        .start_turn(
            app.handle().clone(),
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "leave approval pending".into(),
                effort: Some("none".into()),
                client_message_id: None,
            },
        )
        .await
        .unwrap();
    wait_for_pending_approval(&manager).await;
    let attachment = manager
        .sessions
        .read()
        .await
        .get(&request.session_id)
        .unwrap()
        .clone();
    attachment.server.stop().await.unwrap();
    tokio::time::timeout(SETTLE_TIMEOUT, async {
        loop {
            if manager.approvals.pending_len().await == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("dead origin left an approval pending");
    // Wait on the durable record, not only on the in-memory counter: the broker
    // removes a request from `pending` before it journals the expiry, so
    // `pending_len() == 0` is reached first and reading the journal right then
    // is a race.
    tokio::time::timeout(SETTLE_TIMEOUT, async {
        loop {
            let events = core
                .journal()
                .session_events_after(request.session_id.clone(), 0, 100)
                .await
                .unwrap();
            if events.iter().any(|event| {
                event.kind == "approval.expired"
                    && event.payload["reason"] == "origin_process_exited"
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("dead origin never journalled its approval expiry");
}

#[tokio::test]
async fn killed_app_server_interrupts_sqlite_and_resumes_the_same_thread() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
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
                client_message_id: None,
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
    wait_for_record_status(
        &manager,
        &request.session_id,
        SessionStatus::Interrupted.as_str(),
    )
    .await;
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
    manager
        .interrupt(app.handle().clone(), &request.session_id)
        .await
        .unwrap();

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
                client_message_id: None,
            },
        )
        .await
        .unwrap()
        .turn_id
        .unwrap();
    assert_ne!(first_turn, second_turn);
    let requests = fixture_requests(&codex_root, &request.session_id);
    assert!(requests.iter().any(|message| {
        message["method"] == "thread/resume" && message["params"]["threadId"] == "thread-fixture"
    }));
    manager.close(&request.session_id).await.unwrap();
}

/// Stop is a containment boundary, not a cosmetic state transition. A
/// non-cooperative app-server may acknowledge `turn/interrupt` while its shell
/// tool keeps running, so the Desktop must terminate the attachment's isolated
/// process group, persist the exact terminal state, and permit a fresh turn on
/// the durable thread.
#[cfg(unix)]
#[tokio::test]
async fn interrupt_terminates_non_cooperative_tool_tree_and_allows_a_new_turn() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let request = test_request(temp.path(), "stop-tool-tree");
    let home = codex_root.join("homes").join("stop-tool-tree");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("ignore-interrupt-and-spawn-sleeper"), "1").unwrap();

    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let first_turn = manager
        .start_turn(
            app_handle.clone(),
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "start a tool that ignores cooperative cancellation".into(),
                effort: Some("none".into()),
                client_message_id: None,
            },
        )
        .await
        .unwrap()
        .turn_id
        .unwrap();
    let sleeper_pid = tokio::time::timeout(SETTLE_TIMEOUT, async {
        loop {
            if let Ok(pid) = fs::read_to_string(home.join("sleeping-child.pid")) {
                break pid.trim().parse::<libc::pid_t>().unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fixture tool never started");

    manager
        .interrupt(app_handle.clone(), &request.session_id)
        .await
        .unwrap();
    wait_for_record_status(&manager, &request.session_id, SessionStatus::Ready.as_str()).await;
    wait_for_run_status(&core, &first_turn, RunStatus::Cancelled.as_str()).await;
    let first_run = RunService::new(core.storage().database().clone())
        .get(first_turn.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first_run.outcome.unwrap()["reason"], "operator_cancelled");
    let cancelled = core
        .journal()
        .session_events_after(request.session_id.clone(), 0, 200)
        .await
        .unwrap()
        .into_iter()
        .filter(|event| {
            event.kind == "turn/interrupted"
                && event.payload["turnId"] == first_turn
                && event.payload["reason"] == "operator_cancelled"
                && event.payload["cancelledBy"] == "user"
        })
        .count();
    assert_eq!(
        cancelled, 1,
        "Stop must journal one explicit user cancellation"
    );
    assert!(!manager
        .sessions
        .read()
        .await
        .contains_key(&request.session_id));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if unsafe { libc::kill(sleeper_pid, 0) } != 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Stop left the descendant tool process alive");

    // The process tree is gone, but the conversation is still restartable.
    fs::remove_file(home.join("ignore-interrupt-and-spawn-sleeper")).unwrap();
    let resumed = manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    assert_eq!(resumed.thread_id, "thread-fixture");
    let second_turn = manager
        .start_turn(
            app_handle,
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "start a new turn after Stop".into(),
                effort: Some("none".into()),
                client_message_id: None,
            },
        )
        .await
        .unwrap()
        .turn_id
        .unwrap();
    assert_ne!(first_turn, second_turn);
    let runs = RunService::new(core.storage().database().clone())
        .list_for_session(request.session_id.clone(), 20)
        .await
        .unwrap();
    assert_eq!(
        runs.len(),
        2,
        "Stop + follow-up must create no duplicate run"
    );
    manager.close(&request.session_id).await.unwrap();
}

#[tokio::test]
async fn final_answer_before_app_server_exit_completes_the_run() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "final-answer-exit");

    manager
        .start(app.handle().clone(), request.clone())
        .await
        .unwrap();
    arm_final_answer_then_exit(&codex_root, &request.session_id);
    let turn_id = manager
        .start_turn(
            app.handle().clone(),
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "reply and finish".into(),
                effort: Some("none".into()),
                client_message_id: None,
            },
        )
        .await
        .unwrap()
        .turn_id
        .unwrap();

    wait_for_record_status(&manager, &request.session_id, SessionStatus::Ready.as_str()).await;
    wait_for_run_status(&core, &turn_id, RunStatus::Completed.as_str()).await;
}

#[tokio::test]
async fn steer_turn_sends_turn_steer_with_the_active_turn_id() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
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
                client_message_id: None,
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
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core)),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
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
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
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

fn arm_terminal_before_turn_start_response(root: &Path, session_id: &str) {
    let home = session_home(root, session_id);
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("complete-before-turn-start-response"), "1").unwrap();
}

fn arm_final_answer_then_exit(root: &Path, session_id: &str) {
    let home = session_home(root, session_id);
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("final-answer-then-exit"), "1").unwrap();
}

fn send_request(start: CodexSessionStartRequest, prompt: &str) -> CodexTurnSendRequest {
    CodexTurnSendRequest {
        start,
        prompt: prompt.into(),
        effort: Some("none".into()),
        compact_before_model_switch: false,
        client_message_id: None,
    }
}

#[tokio::test]
async fn turn_send_journals_the_renderer_message_id_once() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root,
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "one-user-message");
    let mut send = send_request(request.clone(), "one logical submission");
    send.client_message_id = Some("user-renderer-1".into());

    manager
        .send_turn(app.handle().clone(), send)
        .await
        .expect("turn starts");

    let user_messages: Vec<_> = core
        .journal()
        .session_events_after(request.session_id.clone(), 0, 200)
        .await
        .expect("session events")
        .into_iter()
        .filter(|event| event.kind == "message.created" && event.payload["role"] == "user")
        .collect();
    assert_eq!(user_messages.len(), 1);
    assert_eq!(user_messages[0].payload["messageId"], "user-renderer-1");
    assert_eq!(
        user_messages[0].payload["content"],
        "one logical submission"
    );

    manager.close(&request.session_id).await.unwrap();
}

/// Switching model mid-conversation replaces the app-server attachment. That is
/// a transport rebind, not the end of the conversation: recording it as a
/// durable `closed` left the next turn unable to create its run, so the turn ran
/// and completed upstream with no run row, no `active_run_id`, and an internal
/// storage string in the composer.
#[tokio::test]
async fn switching_model_mid_conversation_keeps_creating_durable_runs() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root,
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "model-switch-run");
    let sessions = SessionService::new(core.storage().database().clone());
    let runs = RunService::new(core.storage().database().clone());

    let first = manager
        .send_turn(
            app.handle().clone(),
            send_request(request.clone(), "first message on the source model"),
        )
        .await
        .expect("the first turn starts");
    let first_turn = first.turn_id.clone().expect("first turn id");

    // Rebind the same conversation to a different model, exactly as the
    // composer does when the operator switches model mid-thread. Any changed
    // attachment property takes this path; the model is the one the QA
    // reproduction used.
    let mut switched = request.clone();
    switched.model = "poolside/Laguna-M-3.0-mlx".into();
    switched.local_model_catalog = Some(serde_json::json!({
        "models": [model_catalog_entry("poolside/Laguna-M-3.0-mlx", "Destination model instructions.")]
    }));
    let second = manager
        .send_turn(
            app.handle().clone(),
            send_request(switched.clone(), "first message on the destination model"),
        )
        .await
        .expect("the turn after a model switch must not be refused by storage");
    let second_turn = second.turn_id.clone().expect("second turn id");
    assert_ne!(second_turn, first_turn);

    // The destination turn owns a durable run, and the session agrees with it.
    let run = runs
        .get(second_turn.clone())
        .await
        .unwrap()
        .expect("the destination turn has a durable run");
    assert_eq!(run.session_id, request.session_id);
    assert_eq!(run.model.as_deref(), Some("poolside/Laguna-M-3.0-mlx"));
    let session = sessions
        .get(request.session_id.clone())
        .await
        .unwrap()
        .expect("session row");
    assert_eq!(session.status, SessionStatus::Running.as_str());
    assert_eq!(session.active_run_id.as_deref(), Some(second_turn.as_str()));

    // Exactly one run per send, and the rebind never closed the conversation.
    let all = runs
        .list_for_session(request.session_id.clone(), 100)
        .await
        .unwrap();
    assert_eq!(all.len(), 2, "one run per send, no duplicates: {all:?}");
    assert!(!core
        .journal()
        .session_events_after(request.session_id.clone(), 0, 500)
        .await
        .unwrap()
        .iter()
        .any(|event| event.kind == "session.status_changed"
            && event.payload["to"] == SessionStatus::Closed.as_str()));

    manager.close(&request.session_id).await.unwrap();
}

/// Closing a conversation is still durable and terminal — the rebind split must
/// not weaken the real close.
#[tokio::test]
async fn closing_a_session_still_closes_it_durably() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root,
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "explicit-close");

    manager
        .send_turn(
            app.handle().clone(),
            send_request(request.clone(), "one turn then close"),
        )
        .await
        .expect("turn starts");
    manager.close(&request.session_id).await.unwrap();

    assert_eq!(
        SessionService::new(core.storage().database().clone())
            .get(request.session_id.clone())
            .await
            .unwrap()
            .unwrap()
            .status,
        SessionStatus::Closed.as_str()
    );
}

/// Settings and tab navigation can detach a completed chat before the operator
/// approves a capability. Selecting that chat and sending "continue" must
/// reopen the durable session before the provider turn starts; otherwise the
/// upstream turn exists without a run row and surfaces `RunNotPersisted`.
#[tokio::test]
async fn sending_to_a_closed_session_reopens_with_a_durable_run() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root,
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "closed-session-reconnect");

    let first = manager
        .send_turn(
            app.handle().clone(),
            send_request(request.clone(), "request capability"),
        )
        .await
        .expect("first turn starts");
    let first_turn = first.turn_id.expect("first turn id");
    manager.close(&request.session_id).await.unwrap();

    let resumed = manager
        .send_turn(
            app.handle().clone(),
            send_request(request.clone(), "continue after approval"),
        )
        .await
        .expect("closed conversation reconnects");
    let resumed_turn = resumed.turn_id.expect("resumed turn id");
    assert_ne!(resumed_turn, first_turn);

    let sessions = SessionService::new(core.storage().database().clone());
    let session = sessions
        .get(request.session_id.clone())
        .await
        .unwrap()
        .expect("session row");
    assert_eq!(session.status, SessionStatus::Running.as_str());
    assert_eq!(
        session.active_run_id.as_deref(),
        Some(resumed_turn.as_str())
    );

    let runs = RunService::new(core.storage().database().clone());
    let durable = runs
        .get(resumed_turn)
        .await
        .unwrap()
        .expect("resumed turn has a durable run");
    assert_eq!(durable.session_id, request.session_id);

    manager.close(&request.session_id).await.unwrap();
}

#[tokio::test]
async fn terminal_before_run_creation_reconciles_the_exact_turn() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "fast-terminal");
    arm_terminal_before_turn_start_response(&codex_root, &request.session_id);

    let info = manager
        .send_turn(
            app.handle().clone(),
            send_request(request.clone(), "finish before the start response"),
        )
        .await
        .expect("the fast turn is durably reconciled");
    let turn_id = info.turn_id.expect("fixture returns a turn id");
    wait_for_run_status(&core, &turn_id, "completed").await;
    assert_eq!(
        manager.records.read().await[&request.session_id].status,
        SessionStatus::Ready.as_str(),
        "send_turn must return with the cache reconciled, not briefly stuck running"
    );

    let session = core
        .sessions()
        .get(request.session_id.clone())
        .await
        .unwrap()
        .expect("session remains durable");
    assert_eq!(session.active_run_id, None);
    let run = core
        .runs()
        .get(turn_id.clone())
        .await
        .unwrap()
        .expect("run remains durable");
    assert_eq!(run.status, "completed");
    assert_eq!(run.outcome.unwrap()["turn"]["id"], turn_id);
    let events = core
        .journal()
        .session_events_after(request.session_id.clone(), 0, 200)
        .await
        .unwrap();
    let terminal_index = events
        .iter()
        .position(|event| event.kind == "turn/completed")
        .expect("fixture terminal is journalled");
    let run_started_index = events
        .iter()
        .position(|event| event.kind == "run.started")
        .expect("run creation is journalled");
    assert!(
        terminal_index < run_started_index,
        "fixture must exercise terminal-before-run creation"
    );
    assert!(events.iter().any(|event| {
        event.kind == "run.status_changed"
            && event.payload["runId"] == turn_id
            && event.payload["to"] == "completed"
    }));

    manager.close(&request.session_id).await.unwrap();
}

/// The screenshot bug: the app-server exits between attach and turn/start.
/// The event pump owns process-exit finalization and completes it before the
/// renderer gets the typed detachment. A later retry resumes the same thread.
#[tokio::test]
async fn turn_send_reports_detachment_after_event_pump_finalizes_the_run() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
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

    assert_eq!(
        manager.records.read().await[&request.session_id].status,
        SessionStatus::Ready.as_str()
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
    assert_eq!(run.outcome.unwrap()["reason"], "app_server_exited");
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
    manager
        .interrupt(app.handle().clone(), &request.session_id)
        .await
        .unwrap();

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
        message["method"] == "thread/resume" && message["params"]["threadId"] == "thread-fixture"
    }));
    manager.close(&request.session_id).await.unwrap();
}

/// A single exit is absorbed inside the command: the renderer sees one
/// successful send, never a transient error it has to model.
#[tokio::test]
async fn turn_send_retries_once_through_a_dying_app_server() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
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
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    fs::create_dir_all(&codex_root).unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    SessionService::new(core.storage().database().clone())
        .create_or_update(SessionCreate {
            id: "restored-running".into(),
            title: "Restored local turn".into(),
            kind: SessionKind::Codex,
            target: RuntimeTarget::local_laguna(),
            project_id: None,
            remote_id: None,
            codex_thread_id: Some("thread-restored".into()),
            status: SessionStatus::Ready,
            state_generation: None,
            metadata: json!({"titleOrigin":"automatic"}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
    RunService::new(core.storage().database().clone())
        .start(RunCreate {
            id: "orphaned-run".into(),
            session_id: "restored-running".into(),
            mode: "codex_turn".into(),
            model: Some("laguna".into()),
            adapter: None,
            metadata: json!({"threadId":"thread-restored"}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
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
        presentation_emotion: None,
        presentation_summary: None,
        approval_policy: "never".into(),
        sandbox: "workspace-write".into(),
        recovery: None,
    };
    fs::write(
        codex_root.join("threads.json"),
        serde_json::to_vec_pretty(&HashMap::from([("restored-running".to_owned(), record)]))
            .unwrap(),
    )
    .unwrap();

    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
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
    let orphaned = RunService::new(core.storage().database().clone())
        .get("orphaned-run".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(orphaned.status, RunStatus::Interrupted.as_str());
    assert_eq!(orphaned.outcome.unwrap()["reason"], "desktop_reattached");
    let requests = fixture_requests(&codex_root, "restored-running");
    assert!(requests.iter().any(|message| {
        message["method"] == "thread/resume" && message["params"]["threadId"] == "thread-restored"
    }));
    manager.close("restored-running").await.unwrap();
}

#[tokio::test]
async fn rejected_turn_send_arguments_never_mark_the_session_running() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        codex_root,
        fixture_binary(),
        CodexManager::test_broker(),
    );
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
                client_message_id: None,
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
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
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
                client_message_id: None,
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

/// Renderer optimistic bubbles and host journaling must share one message id.
/// Divergent UUIDs were the CUA P1: every submitted prompt rendered twice.
#[tokio::test]
async fn turn_send_reuses_client_message_id_in_journalled_user_prompt() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root,
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let request = test_request(temp.path(), "client-message-id");
    let client_message_id = "user-optimistic-42";

    manager
        .send_turn(
            app.handle().clone(),
            CodexTurnSendRequest {
                start: request.clone(),
                prompt: "one bubble please".into(),
                effort: Some("none".into()),
                compact_before_model_switch: false,
                client_message_id: Some(client_message_id.into()),
            },
        )
        .await
        .expect("turn starts");

    let events = core
        .journal()
        .session_events_after(request.session_id.clone(), 0, 50)
        .await
        .expect("session events");
    let user_prompts: Vec<_> = events
        .iter()
        .filter(|event| {
            event.kind == "message.created"
                && event.payload.get("role").and_then(Value::as_str) == Some("user")
        })
        .collect();
    assert_eq!(
        user_prompts.len(),
        1,
        "expected exactly one journalled user message.created, got {user_prompts:?}"
    );
    assert_eq!(
        user_prompts[0]
            .payload
            .get("messageId")
            .and_then(Value::as_str),
        Some(client_message_id)
    );
    assert_eq!(
        user_prompts[0]
            .payload
            .get("content")
            .and_then(Value::as_str),
        Some("one bubble please")
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
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        codex_root,
        fixture_binary(),
        CodexManager::test_broker(),
    );
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
fn apply_patch_approval_is_policy_resolved_not_rejected() {
    // Before this method was recognized, applyPatchApproval fell through to
    // the -32601 catch-all and failed the turn even under `never`.
    assert!(is_approval_method("applyPatchApproval"));
    let payload = safe_approval_payload(
        "approval-2",
        "applyPatchApproval",
        &json!({"path":"/workspace/src/lib.rs"}),
        &["decline".into(), "accept".into()],
    );
    assert_eq!(payload["detail"], "Modify /workspace/src/lib.rs");
    assert_eq!(payload["alwaysSupported"], false);
}

#[test]
fn automatic_approval_decision_mirrors_the_delivered_response_and_validates() {
    use crate::session::approval::{ApprovalKind, ApprovalScope};
    // The receipt written for a `never` auto-approval must describe exactly
    // the decision the provider was sent, and that decision must be one the
    // approval kind accepts, or the receipt write would fail.
    let session_scoped = vec!["decline".into(), "accept".into(), "acceptForSession".into()];
    let once_only = vec!["decline".into(), "accept".into()];
    let session_decision = automatic_approval_decision(&session_scoped).expect("decision");
    let once_decision = automatic_approval_decision(&once_only).expect("decision");
    assert!(matches!(
        &session_decision,
        crate::session::approval::ApprovalDecision::Approve {
            scope: ApprovalScope::Session
        }
    ));
    assert!(matches!(
        &once_decision,
        crate::session::approval::ApprovalDecision::Approve {
            scope: ApprovalScope::Once
        }
    ));
    assert!(automatic_approval_decision(&["decline".into()]).is_none());
    let session_kind = ApprovalKind::ShellCommand {
        request_method: "execCommandApproval".into(),
        detail: "Run a shell command".into(),
        scope: None,
        always_supported: true,
    };
    let once_kind = ApprovalKind::ShellCommand {
        request_method: "applyPatchApproval".into(),
        detail: "Modify workspace files".into(),
        scope: None,
        always_supported: false,
    };
    session_kind.validate_decision(&session_decision).unwrap();
    once_kind.validate_decision(&once_decision).unwrap();
}

#[test]
fn approval_request_variants_and_nested_decisions_are_normalized() {
    assert!(is_approval_method("item/commandExecution/requestApproval"));
    assert!(is_approval_method("item/commandExecution/request_approval"));
    assert!(is_approval_method("permissions/request"));
    assert!(!is_approval_method("item/commandExecution/started"));
    assert_eq!(
        approval_decisions(&json!({"item":{"available_decisions":["decline","accept"]}})),
        vec!["decline", "accept"]
    );
}
#[tokio::test]
async fn app_server_approval_is_journaled_and_resumes_after_one_approval() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("untrusted", "workspace-write");
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let mut request = test_request(temp.path(), "approval-round-trip");
    request.approval_policy = Some("untrusted".into());
    let home = codex_root.join("homes").join("approval-round-trip");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("request-approval-on-turn-start"), "1").unwrap();
    manager
        .start(app.handle().clone(), request.clone())
        .await
        .unwrap();
    manager
        .start_turn(
            app.handle().clone(),
            CodexTurnStartRequest {
                session_id: request.session_id.clone(),
                prompt: "request a shell approval".into(),
                effort: Some("none".into()),
                client_message_id: None,
            },
        )
        .await
        .unwrap();

    let approval = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = core
                .journal()
                .session_events_after(request.session_id.clone(), 0, 100)
                .await
                .unwrap();
            if let Some(event) = events
                .into_iter()
                .find(|event| event.kind == "approval.requested")
            {
                break event;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("approval should be journaled");
    let approval_id = approval.payload["approvalId"].as_str().unwrap().to_owned();
    // The fixture requests approval for the isolated child-home parent. The
    // journal must describe that exact requested scope; substituting a generic
    // workspace would misrepresent where the approved command can run.
    let requested_scope = home.parent().unwrap().display().to_string();
    assert_eq!(
        approval.payload["detail"],
        format!("Run a shell command in {requested_scope}")
    );
    assert_eq!(approval.payload["scope"], requested_scope);
    assert!(!approval.payload.to_string().contains("hidden-command"));

    manager
        .resolve_approval(
            app.handle().clone(),
            super::proto::CodexApprovalDecisionRequest {
                session_id: request.session_id.clone(),
                approval_id,
                decision: "once".into(),
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !home.join("approval-response.json").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("approval response should reach the app server");
    let response: Value =
        serde_json::from_str(&fs::read_to_string(home.join("approval-response.json")).unwrap())
            .unwrap();
    assert_eq!(response["result"]["decision"], "accept");
    manager.close(&request.session_id).await.unwrap();
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
    assert!(
        config.contains("writable_roots = [\"/Users/example/Documents/GitHub\", \"/tmp/a\\\"b\"]")
    );
}
#[test]
fn workspace_write_config_does_not_invent_a_read_denylist_field() {
    let config = workspace_write_config(&["/tmp".into()]);
    assert!(config.contains("[sandbox_workspace_write]"));
    assert!(config.contains("writable_roots"));
    assert!(!config.contains("read_deny"));
    assert!(!config.contains("denied_read"));
    assert!(!config.contains("exclude_globs"));
    let comment = credential_read_policy_comment();
    assert!(comment.contains(".env"));
    assert!(comment.contains("no sandbox read-denylist"));
    assert!(!comment.contains("writable_roots"));
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
        "enabled_tools = [\"optimizer_manage\", \"optimizer_stage_eval_candidates\", \"optimizer_start_recipe\"]\n"
    );
    assert_eq!(
        mcp_enabled_tools("synth_session"),
        "enabled_tools = [\"session_present\"]\n"
    );
    assert_eq!(
        mcp_enabled_tools("synth_secrets"),
        "enabled_tools = [\"secrets_manage\"]\n"
    );
}

#[test]
fn materializes_diagram_skill_with_direct_tool_first_contract() {
    let temp = tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let home = temp.path().join("home");
    ensure_home(&home, &test_request(&workspace, "diagram-skill")).unwrap();
    let skill = fs::read_to_string(home.join("skills/author-synth-diagrams/SKILL.md")).unwrap();
    assert!(skill.contains("Do not call `resources/list` or `resources/read`"));
    assert!(skill.contains("does not require template discovery"));
    assert!(skill.contains("`operation: \"create\"`"));
    assert!(skill.contains("tools.mcp__synth_visuals__visual_manage"));
    assert!(skill.contains("diagram.systems.v1"));
    assert!(skill.contains("diagram.systems.dynamic.v1"));
    assert!(skill.contains("broad topology"));
    assert!(skill.contains("exact call order"));
    assert!(skill.contains("Write a storyboard of at least three named beats"));
    assert!(skill.contains("delegate storyboard plus scene/timeline authoring"));
    assert!(
        skill.contains("The parent agent owns evidence selection, integration, safety validation")
    );
    assert!(skill.contains("Workshop's subagent is the coding collaborator"));
    assert!(!skill.contains("Cursor"));
    assert!(!skill.contains("\"method\": \"visual_manage\""));

    let systems =
        fs::read_to_string(home.join("skills/author-synth-diagrams/references/systems-map.md"))
            .unwrap();
    assert!(systems.contains("template_id: \"diagram.systems.v1\""));
    assert!(systems.contains("explicit finite rectangle"));

    let dynamic =
        fs::read_to_string(home.join("skills/author-synth-diagrams/references/dynamic-systems.md"))
            .unwrap();
    assert!(dynamic.contains("diagram.systems.dynamic.v1"));
    assert!(dynamic.contains("Poster fallback"));
    assert!(dynamic.contains("pause, replay, scrub"));
    assert!(dynamic.contains("\"posterTimeMs\": 9000"));
    assert!(dynamic.contains("\"caption\": \"Retries accumulate\""));
    assert!(dynamic.contains("\"target\": \"queue\""));
    assert!(dynamic.contains("\"durationMs\": 900, \"easing\": \"ease-in-out\""));
    assert!(dynamic
        .contains("`linear`, `ease-in`, `ease-out`, `ease-in-out`, `step-start`, or `step-end`"));
    assert!(dynamic.contains("\"reducedMotion\": \"poster\""));
    assert!(!dynamic.contains("\"poster\":"));
    assert!(!dynamic.contains("Cursor"));

    let visual_skill = fs::read_to_string(home.join("skills/use-synth-visuals/SKILL.md")).unwrap();
    assert!(visual_skill.contains("tools.mcp__synth_visuals__visual_manage"));
    assert!(visual_skill.contains("diagram.systems.v1"));
    assert!(visual_skill.contains("diagram.systems.dynamic.v1"));
    assert!(visual_skill.contains("There is\n**no** top-level `method` field"));
    assert!(!visual_skill.contains("`method: \"visual_manage\"`"));
    assert!(!visual_skill.contains("{\"method\":\"visual_manage\""));

    let session_skill = fs::read_to_string(home.join("skills/use-synth-session/SKILL.md")).unwrap();
    let optimizers_skill =
        fs::read_to_string(home.join("skills/use-synth-optimizers/SKILL.md")).unwrap();
    assert_eq!(
        optimizers_skill,
        fs::read_to_string(home.join("skills/.system/use-synth-optimizers/SKILL.md")).unwrap()
    );
    assert_eq!(
        session_skill,
        fs::read_to_string(home.join("skills/.system/use-synth-session/SKILL.md")).unwrap()
    );
    assert!(session_skill.contains("tools.mcp__synth_session__session_present"));
    assert!(session_skill.contains("seven"));
    assert!(session_skill.contains("Manual"));
    let secrets_skill = fs::read_to_string(home.join("skills/use-synth-secrets/SKILL.md")).unwrap();
    assert_eq!(
        secrets_skill,
        fs::read_to_string(home.join("skills/.system/use-synth-secrets/SKILL.md")).unwrap()
    );
    assert!(secrets_skill.contains("tools.mcp__synth_secrets__secrets_manage"));
    assert!(secrets_skill.contains("request_env_import"));
    assert!(secrets_skill.contains("Codex sandbox cannot deny those reads"));
    assert!(!secrets_skill.contains("secrets_create"));
    let agents = fs::read_to_string(home.join("AGENTS.md")).unwrap();
    assert!(agents.contains(".env"));
    assert!(agents.contains("no read-denylist field"));
    let generated = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(generated.contains("no sandbox read-denylist"));
    assert!(!generated.contains("read_deny"));
    assert!(!generated.contains("denied_read"));
    assert!(optimizers_skill.contains("mcp__synth_session__session_present"));
    assert!(optimizers_skill.contains("run ID's final 6 characters"));

    let computer_use_skill =
        fs::read_to_string(home.join("skills/use-computer-use/SKILL.md")).unwrap();
    assert_eq!(
        computer_use_skill,
        fs::read_to_string(home.join("skills/.system/use-computer-use/SKILL.md")).unwrap()
    );
    assert!(computer_use_skill.contains("com.apple.Safari"));
    assert!(computer_use_skill.contains("Never research the tool contract"));
    assert!(computer_use_skill.contains("Do not invent verbs"));
    assert!(computer_use_skill.contains("press_key"));
    assert!(computer_use_skill.contains("key: \"cmd+n\""));
    assert!(computer_use_skill.contains("Only now call `get_app_state`"));
    let browser_skill =
        fs::read_to_string(home.join("skills/use-workshop-browser/SKILL.md")).unwrap();
    assert!(browser_skill.contains("browser_snapshot"));
    assert!(browser_skill.contains("20,000"));
    assert!(browser_skill.contains("explicitly requested Safari"));
}
#[test]
fn generated_mcp_configs_use_each_adapter_owned_ipc_variable() {
    assert_eq!(mcp_ipc_env_key("synth_visuals"), "SYNTH_VISUALS_IPC_FILE");
    assert_eq!(
        mcp_ipc_env_key("synth_containers"),
        "SYNTH_DESKTOP_IPC_FILE"
    );
    assert_eq!(
        mcp_ipc_env_key("synth_optimizers"),
        "SYNTH_DESKTOP_IPC_FILE"
    );
    assert_eq!(mcp_ipc_env_key("synth_session"), "SYNTH_DESKTOP_IPC_FILE");
}
#[test]
fn generated_mcp_configs_pass_desktop_identity_for_visual_capture() {
    let config = mcp_env_config(
        "synth_visuals",
        Path::new("/tmp/visuals-ipc.json"),
        "session-123",
        "Synth Workshop v0.4 · cua",
        "com.synth.desktop.v04.dev.cua",
    );
    assert!(config.contains("SYNTH_VISUALS_IPC_FILE = \"/tmp/visuals-ipc.json\""));
    assert!(config.contains("SYNTH_SESSION_ID = \"session-123\""));
    assert!(config.contains("SYNTH_DESKTOP_APP_NAME = \"Synth Workshop v0.4 · cua\""));
    assert!(config.contains("SYNTH_DESKTOP_BUNDLE_ID = \"com.synth.desktop.v04.dev.cua\""));
}
#[test]
fn generated_browser_config_passes_human_owned_policy_and_profile_paths() {
    let config = mcp_env_config(
        "synth_browser",
        Path::new("/tmp/desktop-ipc.json"),
        "session-123",
        "Synth Workshop v0.5 · cua",
        "com.synth.desktop.v05.dev.cua",
    );
    assert!(config.contains("SYNTH_BROWSER_POLICY_FILE"));
    assert!(config.contains("browser/policy.json"));
    assert!(config.contains("SYNTH_BROWSER_PROFILE_ROOT"));
    assert!(config.contains("browser-profiles"));
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
    let (broker, _listener) =
        CredentialBroker::bind(std::sync::Arc::new(credential_broker::ReceiptStore::new()))
            .unwrap();
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
    assert!(config.contains(&format!("base_url = \"{}/api/v1\"", broker.origin())));
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
    // Every reference the skill links must exist in the home, or an agent is
    // told to read a file it cannot open.
    for name in ["gepa.md", "gelo.md", "eval.md", "sft.md"] {
        assert!(
            optimizer_skill.contains(&format!("references/{name}")),
            "SKILL.md should link {name}"
        );
        assert!(
            home.join("skills/use-synth-optimizers/references")
                .join(name)
                .is_file(),
            "{name} should be installed beside SKILL.md"
        );
    }
    assert!(optimizer_skill.contains("If the first `await_ready` reports"));
    assert!(optimizer_skill.contains("Do not inspect processes"));
    assert!(optimizer_skill.contains("on the first `start` call"));
    assert!(optimizer_skill.contains("Never run a shell or terminal command, including `sleep`"));
    let optimizer_eval_reference =
        fs::read_to_string(home.join("skills/use-synth-optimizers/references/eval.md")).unwrap();
    assert!(optimizer_eval_reference.contains("candidate_set_id"));
    assert!(optimizer_eval_reference.contains("stage_eval_candidates"));
    let visuals_skill = fs::read_to_string(home.join("skills/use-synth-visuals/SKILL.md")).unwrap();
    assert!(visuals_skill.contains("Optimizer visuals are a strict exception"));
}

#[test]
fn codex_oauth_materializes_private_chatgpt_auth_without_child_env() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("oauth-home");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let mut request = test_request(&workspace, "oauth-materialize");
    request.provider_name = Some(crate::codex_oauth::PROVIDER_ID.into());
    request.provider_title = Some("ChatGPT subscription (Codex OAuth)".into());
    request.provider_env_key = None;
    request.base_url = "https://chatgpt.com/backend-api/codex".into();
    request.model = "gpt-5.6-luna".into();
    request.api_key = serde_json::to_string(&crate::codex_oauth::Credential {
        access_token: "access-secret".into(),
        refresh_token: "refresh-secret".into(),
        id_token: "id-secret".into(),
        expires_ms: 2_000_000_000_000,
        account_id: "acct_123".into(),
        account_hint: None,
        last_refresh_ms: 1_700_000_000_000,
    })
    .unwrap();

    ensure_home(&home, &request).unwrap();
    let config = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(config.contains("requires_openai_auth = true"));
    assert!(config.contains("cli_auth_credentials_store = \"file\""));
    assert!(config.contains("base_url = \"https://chatgpt.com/backend-api/codex\""));
    assert!(!config.contains("env_key ="));
    assert!(!config.contains("access-secret"));
    let auth = fs::read_to_string(home.join("auth.json")).unwrap();
    assert!(auth.contains("\"auth_mode\": \"chatgpt\""));
    assert!(auth.contains("access-secret"));
    assert!(!auth.contains("refresh-secret"));
    assert!(auth.contains("synth-desktop-does-not-delegate-refresh"));
    assert_eq!(provider_child_env(&request).unwrap(), None);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(home.join("auth.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
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
    let local_laguna_config = fs::read_to_string(local_laguna_home.join("config.toml")).unwrap();
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
    assert!(
        validate_start(&request).is_ok(),
        "a persisted preference is capped to the model-safe maximum"
    );
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
fn caps_a_shared_compaction_preference_for_smaller_codex_contexts() {
    let temp = tempdir().unwrap();
    let mut request = test_request(temp.path(), "compact-terra");
    request.model = "openai/gpt-5.6-terra".into();
    request.auto_compact_token_limit = Some(250_000);

    assert_eq!(auto_compact_token_limit(&request), 235_929);
    assert!(validate_start(&request).is_ok());
}

#[test]
fn writes_selected_codex_service_tier() {
    let temp = tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let mut request = test_request(&workspace, "fast-tier");
    request.service_tier = Some("fast".into());
    validate_start(&request).unwrap();
    let home = temp.path().join("home");
    ensure_home(&home, &request).unwrap();
    assert!(fs::read_to_string(home.join("config.toml"))
        .unwrap()
        .contains("service_tier = \"fast\""));
    request.service_tier = Some("turbo".into());
    assert!(validate_start(&request).is_err());
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
    let (broker, _listener) =
        CredentialBroker::bind(std::sync::Arc::new(credential_broker::ReceiptStore::new()))
            .unwrap();
    let mut request = test_request(temp.path(), "synth-cloud-overwrite");
    request.api_key = "renderer-leaked-key".into();
    request.base_url = "https://evil.example/v1".into();
    apply_synth_cloud_provider(
        &mut request,
        "http://127.0.0.1:41209/",
        Some("sk_dev_real_key"),
    )
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
fn openrouter_provider_overwrites_renderer_endpoint_before_leasing() {
    let temp = tempdir().unwrap();
    let (broker, _listener) =
        CredentialBroker::bind(std::sync::Arc::new(credential_broker::ReceiptStore::new()))
            .unwrap();
    let mut request = test_request(temp.path(), "openrouter-origin-custody");
    request.provider_name = Some("openrouter".into());
    request.base_url = "https://credential-thief.example/api/v1".into();
    request.api_key = "renderer-value".into();

    apply_openrouter_provider(&mut request, Some("sk-or-native")).unwrap();
    assert_eq!(request.base_url, OPENROUTER_RESPONSES_BASE_URL);
    assert_eq!(request.api_key, "sk-or-native");
    assert!(request.broker_credential);

    apply_brokered_credential(&mut request, &broker).unwrap();
    assert_eq!(request.base_url, format!("{}/api/v1", broker.origin()));
    assert_eq!(
        broker.upstream_for(&request.api_key).as_deref(),
        Some("https://openrouter.ai")
    );
}

#[test]
fn synth_cloud_normalizes_a_local_bind_address_for_the_client() {
    let temp = tempdir().unwrap();
    let (broker, _listener) =
        CredentialBroker::bind(std::sync::Arc::new(credential_broker::ReceiptStore::new()))
            .unwrap();
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
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        temp.path().join("codex"),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "lease-live-reuse");
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_reuse"))
        .unwrap();

    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let broker = manager.broker.clone();
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
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        temp.path().join("codex"),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "lease-rebind");
    apply_synth_cloud_provider(
        &mut request,
        "http://127.0.0.1:41209",
        Some("sk_dev_rebind"),
    )
    .unwrap();
    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let broker = manager.broker.clone();
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
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        temp.path().join("codex"),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "lease-provider-identity");
    apply_synth_cloud_provider(
        &mut request,
        "http://127.0.0.1:41209",
        Some("sk_dev_shared"),
    )
    .unwrap();
    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let broker = manager.broker.clone();
    let before = broker.token_for("lease-provider-identity").unwrap();
    manager.receipts().push(credential_broker::SettledReceipt {
        session_id: "lease-provider-identity".into(),
        turn_scope: None,
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
        manager
            .receipts()
            .drain("lease-provider-identity")
            .is_empty(),
        "receipts born under the old provider name must not survive the switch"
    );
}

/// A changed credential or endpoint is part of the reuse comparison: the
/// old child was spawned against the old binding, so rotation must respawn
/// it rather than leave it talking through the stale credential.
#[tokio::test]
async fn a_rotated_credential_respawns_the_child_with_a_fresh_lease() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        temp.path().join("codex"),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let mut request = test_request(temp.path(), "lease-rotation");
    apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_old")).unwrap();
    manager
        .start(app_handle.clone(), request.clone())
        .await
        .unwrap();
    let broker = manager.broker.clone();
    let old_token = broker.token_for("lease-rotation").unwrap();
    let old_attachment = manager
        .sessions
        .read()
        .await
        .get("lease-rotation")
        .unwrap()
        .attachment_id;

    let mut rotated = test_request(temp.path(), "lease-rotation");
    apply_synth_cloud_provider(&mut rotated, "http://127.0.0.1:41209", Some("sk_dev_new")).unwrap();
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

#[tokio::test]
async fn a_refreshed_chatgpt_access_token_rebinds_without_delegating_refresh() {
    let _machine =
        crate::synth_config::test_machine_permissions::install("never", "workspace-write");
    let temp = tempdir().unwrap();
    let root = temp.path().join("codex");
    let manager = CodexManager::with_paths(
        SessionPersistence::Null,
        root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();
    let request_with = |access: &str| {
        let mut request = test_request(temp.path(), "oauth-rotation");
        request.provider_name = Some(crate::codex_oauth::PROVIDER_ID.into());
        request.provider_title = Some("ChatGPT subscription (Codex OAuth)".into());
        request.base_url = "https://chatgpt.com/backend-api/codex".into();
        request.api_key = serde_json::to_string(&crate::codex_oauth::Credential {
            access_token: access.into(),
            refresh_token: "native-refresh-secret".into(),
            id_token: "bounded-id-token".into(),
            expires_ms: 2_000_000_000_000,
            account_id: "acct_test".into(),
            account_hint: None,
            last_refresh_ms: 1_700_000_000_000,
        })
        .unwrap();
        request
    };

    manager
        .start(app_handle.clone(), request_with("old-access"))
        .await
        .unwrap();
    let old_attachment = manager
        .sessions
        .read()
        .await
        .get("oauth-rotation")
        .unwrap()
        .attachment_id;

    manager
        .start(app_handle, request_with("new-access"))
        .await
        .unwrap();
    let new_attachment = manager
        .sessions
        .read()
        .await
        .get("oauth-rotation")
        .unwrap()
        .attachment_id;
    assert_ne!(old_attachment, new_attachment);
    let auth = fs::read_to_string(root.join("homes/oauth-rotation/auth.json")).unwrap();
    assert!(auth.contains("new-access"));
    assert!(!auth.contains("old-access"));
    assert!(!auth.contains("native-refresh-secret"));
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
    request.base_url = "http://user:secret-token@0.0.0.0:41209/api/v1?api_key=secret-token".into();

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
    let (broker, _listener) =
        CredentialBroker::bind(std::sync::Arc::new(credential_broker::ReceiptStore::new()))
            .unwrap();
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

    let (broker, _listener) =
        CredentialBroker::bind(std::sync::Arc::new(credential_broker::ReceiptStore::new()))
            .unwrap();
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
    fs::write(
        snapshots.join("snapshot.sh"),
        format!("#!/bin/sh\n{exported}"),
    )
    .unwrap();
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
        automatic_thread_title("please add session descriptions to the Rust core. Then test it"),
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

fn tracker_for(session_id: &str, provider: &str, turn_id: &str) -> TurnPerformanceTracker {
    TurnPerformanceTracker {
        segments: TurnSegmentTracker::new(session_id, turn_id, Some(provider.into()), None),
        provider: provider.into(),
        model_id: "openrouter/poolside/laguna-s-2.1".into(),
        turn_id: turn_id.into(),
        receipt_scope: turn_id.into(),
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
        turn_scope: Some("turn-1".into()),
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

async fn finalize_turn(
    core: &Arc<CoreRuntime>,
    receipts: &credential_broker::ReceiptStore,
    session_id: &str,
    provider: &str,
    turn: &str,
) {
    let trackers: PerformanceTrackers = Arc::default();
    trackers.lock().await.insert(
        session_id.to_owned(),
        tracker_for(session_id, provider, turn),
    );
    finalize_performance_tracker(
        &SessionPersistence::from_core(Some(core.clone())),
        &trackers,
        receipts,
        session_id,
        "completed",
        Some(2_000),
    )
    .await;
}

async fn usage_totals(core: &Arc<CoreRuntime>) -> UsageBreakdown {
    UsageRecordsRepository::new(core.storage().database().clone())
        .summary("all".into(), None, 0)
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
    let receipts = credential_broker::ReceiptStore::new();
    let session = "wp4-cloud-settles";
    // One turn may make several upstream requests; their settled charges
    // sum, and a token-only receipt contributes no invented money.
    receipts.push(settled_receipt(session, "resp-1", Some(0.01)));
    receipts.push(settled_receipt(session, "resp-2", Some(0.02)));
    receipts.push(settled_receipt(session, "resp-3", None));
    finalize_turn(&core, &receipts, session, "synth-cloud", "turn-1").await;

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
    assert!(receipts.drain(session).is_empty());
}

#[tokio::test]
async fn cloud_receipts_without_money_leave_billed_unset() {
    let temp = tempdir().unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let receipts = credential_broker::ReceiptStore::new();
    let session = "wp4-cloud-token-only";
    receipts.push(settled_receipt(session, "resp-1", None));
    finalize_turn(&core, &receipts, session, "synth-cloud", "turn-1").await;

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
    let receipts = credential_broker::ReceiptStore::new();
    let session = "wp4-local-untouched";
    // Even a stray receipt under a local session's id must not turn an
    // on-device row into a billed one — billed stays None, never $0.
    receipts.push(settled_receipt(session, "resp-1", Some(0.42)));
    finalize_turn(&core, &receipts, session, "local-laguna", "turn-1").await;

    let totals = usage_totals(&core).await;
    assert_eq!(totals.requests, 1);
    assert_eq!(totals.billed_cost_usd, None);
    assert_eq!(totals.estimated_cost_usd, None);
    assert_eq!(totals.cost_source, CostSource::None);
    // The local finalize did not consume the queue.
    assert_eq!(receipts.drain(session).len(), 1);
}

/// A cancellation-race receipt retains its original turn scope and can never
/// be charged to the next turn merely because that turn finalized later.
#[tokio::test]
async fn a_late_receipt_is_never_misattributed_to_the_next_turn() {
    let temp = tempdir().unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let receipts = credential_broker::ReceiptStore::new();
    let session = "wp4-late-receipt";
    finalize_turn(&core, &receipts, session, "synth-cloud", "turn-1").await;
    let totals = usage_totals(&core).await;
    assert_eq!((totals.requests, totals.billed_cost_usd), (1, None));

    receipts.push(settled_receipt(session, "resp-late", Some(0.05)));
    // Still exactly one row: a queued receipt is not a usage record.
    assert_eq!(usage_totals(&core).await.requests, 1);

    finalize_turn(&core, &receipts, session, "synth-cloud", "turn-2").await;
    let totals = usage_totals(&core).await;
    assert_eq!(totals.requests, 2);
    assert_eq!(totals.billed_cost_usd, None);
    assert_eq!(totals.cost_source, CostSource::None);
    assert_eq!(receipts.drain(session).len(), 1);
}

/// The renderer lists Codex chats from this JSON cache, not from SQLite, so a
/// database-only reconciliation still left the sidebar showing Working. The
/// cache must be corrected before `list()` can be called at all.
#[tokio::test]
async fn a_running_record_left_by_a_dead_process_never_lists_as_running() {
    let temp = tempdir().unwrap();
    let codex_root = temp.path().join("codex");
    fs::create_dir_all(&codex_root).unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let sessions = SessionService::new(core.storage().database().clone());
    let mut records = HashMap::new();
    for seed in 201..=205 {
        let session_id = format!("crashed-{seed}");
        sessions
            .create_or_update(SessionCreate {
                id: session_id.clone(),
                title: format!("Craftax seed {seed}"),
                kind: SessionKind::Codex,
                target: RuntimeTarget::from_codex_provider("openrouter", "gpt-5.6-luna"),
                project_id: None,
                remote_id: None,
                codex_thread_id: Some(format!("thread-{seed}")),
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({"titleOrigin": "automatic"}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        RunService::new(core.storage().database().clone())
            .start(RunCreate {
                id: format!("turn-{seed}"),
                session_id: session_id.clone(),
                mode: "codex_turn".into(),
                model: Some("gpt-5.6-luna".into()),
                adapter: None,
                metadata: json!({"threadId": format!("thread-{seed}")}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        records.insert(
            session_id.clone(),
            CodexSessionRecord {
                session_id,
                thread_id: format!("thread-{seed}"),
                workspace: temp.path().display().to_string(),
                model: "gpt-5.6-luna".into(),
                provider_name: "openrouter".into(),
                provider_title: "OpenRouter Responses".into(),
                base_url: "https://openrouter.ai/api/v1".into(),
                status: SessionStatus::Running.as_str().into(),
                title: Some(format!("Craftax seed {seed}")),
                title_origin: Some("automatic".into()),
                presentation_emotion: None,
                presentation_summary: None,
                approval_policy: "never".into(),
                sandbox: "workspace-write".into(),
                recovery: None,
            },
        );
    }
    fs::write(
        codex_root.join("threads.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .unwrap();

    // One chat had actually claimed its turn before the crash; the other four
    // died between `turn/start` and the claim. Both shapes must recover.
    core.storage()
        .database()
        .transaction(|conn| {
            crate::recovery::ownership::claim(
                conn,
                "crashed-201",
                "turn-201",
                "inst_previous_process",
                Some("attach-dead"),
                0,
                chrono::Utc::now(),
            )
        })
        .unwrap();
    drop(core);

    // The relaunch, through the production path: reconciliation happens inside
    // CoreRuntime::open, before anything can read a session, and the manager is
    // built afterward exactly as `setup` builds it.
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let manager = CodexManager::with_paths(
        SessionPersistence::from_core(Some(core.clone())),
        codex_root.clone(),
        fixture_binary(),
        CodexManager::test_broker(),
    );

    let listed = manager.list().await;
    assert_eq!(listed.len(), 5);
    for record in &listed {
        assert_eq!(
            record.status,
            SessionStatus::Interrupted.as_str(),
            "{} still lists as running",
            record.session_id
        );
        let notice = record
            .recovery
            .as_ref()
            .unwrap_or_else(|| panic!("{} lost its recovery notice", record.session_id));
        assert_eq!(notice.reason, "workshop_restarted");
        assert!(notice.restartable);
        assert_eq!(notice.recovery_attempt, 1);
    }
    let claimed = listed
        .iter()
        .find(|record| record.session_id == "crashed-201")
        .unwrap();
    assert_eq!(
        claimed
            .recovery
            .as_ref()
            .unwrap()
            .previous_owner_instance_id
            .as_deref(),
        Some("inst_previous_process")
    );

    // The correction is durable: a second process reading the same file sees it.
    let reread: HashMap<String, CodexSessionRecord> =
        serde_json::from_slice(&fs::read(codex_root.join("threads.json")).unwrap()).unwrap();
    assert!(reread
        .values()
        .all(|record| record.status == SessionStatus::Interrupted.as_str()));
}

// ---- observed generation TPS, measured per output-text segment ----
//
// Fixtures are frozen protocol shapes, replayed through the same normalizer the
// pump uses. `us` is the monotonic receipt clock; nothing here depends on wall
// time, so a replay is deterministic.

/// One Codex `item/agentMessage/delta`, optionally carrying the provider's
/// exact running token count for that item.
fn answer_delta(item: &str, text: &str, cumulative_tokens: Option<i64>) -> Value {
    let mut params = json!({"delta": text, "itemId": item, "turnId": "turn-1"});
    if let Some(tokens) = cumulative_tokens {
        params["cumulativeOutputTokens"] = json!(tokens);
    }
    params
}

fn answer_started(item: &str, phase: &str) -> Value {
    json!({"item": {"id": item, "type": "agentMessage", "phase": phase, "text": ""}})
}

fn answer_completed(item: &str, phase: &str, text: &str) -> Value {
    json!({"item": {"id": item, "type": "agentMessage", "phase": phase, "text": text}})
}

fn tool_started(item: &str) -> Value {
    json!({"item": {"id": item, "type": "commandExecution", "command": "ls", "status": "inProgress"}})
}

/// Replay `(method, params, receipt_microseconds)` through a fresh tracker.
fn replay(
    events: &[(&str, Value, i64)],
) -> Vec<super::generation_speed::GenerationSpeedMeasurement> {
    let mut tracker = TurnSegmentTracker::new("sess", "turn-1", Some("synth-cloud".into()), None);
    for (method, params, at_us) in events {
        if let Some(event) = protocol_event(method, params) {
            tracker.observe(event, *at_us);
        }
    }
    tracker.finish();
    tracker.measurements().to_vec()
}

/// A clean answer segment: four deltas, exact cumulative counts, one second.
fn clean_segment(item: &str, start_us: i64) -> Vec<(&'static str, Value, i64)> {
    vec![
        (
            "item/started",
            answer_started(item, "final_answer"),
            start_us,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "a", Some(10)),
            start_us,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "b", Some(30)),
            start_us + 400_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "c", Some(50)),
            start_us + 800_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "d", Some(70)),
            start_us + 1_200_000,
        ),
        (
            "item/completed",
            answer_completed(item, "final_answer", "abcd"),
            start_us + 1_200_000,
        ),
    ]
}

#[test]
fn regression_slope_matches_the_exact_tokens_of_one_segment() {
    // 60 tokens delivered across 1.2 s after the origin sample: 50 tok/s, and
    // the origin's own 10 tokens are deliberately not in the numerator.
    let measurements = replay(&clean_segment("msg_a", 0));
    let [measurement] = measurements.as_slice() else {
        panic!("expected exactly one segment, got {}", measurements.len());
    };
    assert!(
        (measurement.tps.unwrap() - 50.0).abs() < 1e-9,
        "{:?}",
        measurement.tps
    );
    assert_eq!(measurement.exact_tokens_after_first_sample, 60);
    assert_eq!(measurement.sample_count, 4);
    assert_eq!(measurement.duration_ms, 1_200.0);
    assert_eq!(measurement.status, SegmentStatus::Completed);
    assert_eq!(measurement.phase, SegmentPhase::FinalAnswer);
    assert_eq!(
        measurement.token_count_source,
        TokenCountSource::ProviderItemUsage
    );
    assert_eq!(measurement.unavailable_reason, None);
    // The samples that produced the number travel with it.
    assert_eq!(measurement.samples.len(), 4);
}

#[test]
fn ordinary_least_squares_needs_spread_and_reports_tokens_per_second() {
    assert_eq!(ols_tokens_per_second(&[]), None);
    assert_eq!(ols_tokens_per_second(&[(0.0, 0.0)]), None);
    // Every point at one instant: a burst, not a rate.
    assert_eq!(ols_tokens_per_second(&[(0.0, 0.0), (0.0, 9.0)]), None);
    let rate = ols_tokens_per_second(&[(0.0, 0.0), (1_000_000.0, 25.0)]).unwrap();
    assert!((rate - 25.0).abs() < 1e-9);
}

#[test]
fn a_burst_a_short_span_and_a_thin_segment_all_refuse_to_publish() {
    let one_delta = replay(&[
        ("item/started", answer_started("msg_b", "final_answer"), 0),
        (
            "item/agentMessage/delta",
            answer_delta("msg_b", "hi", Some(40)),
            0,
        ),
        (
            "item/completed",
            answer_completed("msg_b", "final_answer", "hi"),
            10_000,
        ),
    ]);
    assert_eq!(one_delta[0].tps, None);
    assert_eq!(one_delta[0].status, SegmentStatus::Unavailable);
    assert_eq!(
        one_delta[0].unavailable_reason,
        Some(UnavailableReason::InsufficientSamples)
    );

    // Four samples, but only 300 ms of them.
    let mut fast = clean_segment("msg_c", 0);
    for (index, event) in fast.iter_mut().enumerate() {
        event.2 = index as i64 * 100_000;
    }
    let fast = replay(&fast);
    assert_eq!(fast[0].tps, None);
    assert_eq!(
        fast[0].unavailable_reason,
        Some(UnavailableReason::InsufficientDuration)
    );

    // Long enough, but only five tokens arrived after the origin sample.
    let thin = replay(&[
        (
            "item/agentMessage/delta",
            answer_delta("msg_d", "a", Some(10)),
            0,
        ),
        (
            "item/agentMessage/delta",
            answer_delta("msg_d", "b", Some(11)),
            400_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta("msg_d", "c", Some(13)),
            800_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta("msg_d", "d", Some(15)),
            1_200_000,
        ),
        (
            "item/completed",
            answer_completed("msg_d", "final_answer", "abcd"),
            1_200_000,
        ),
    ]);
    assert_eq!(thin[0].tps, None);
    assert_eq!(
        thin[0].unavailable_reason,
        Some(UnavailableReason::InsufficientTokens)
    );
}

#[test]
fn a_five_second_silence_inside_an_open_part_stays_in_the_denominator() {
    // The protocol never said this content part ended, so the quiet stretch is
    // generation time. The deleted 2-second rule would have discarded it and
    // roughly tripled the reported rate.
    let item = "msg_pause";
    let measurements = replay(&[
        ("item/started", answer_started(item, "final_answer"), 0),
        (
            "item/agentMessage/delta",
            answer_delta(item, "a", Some(0)),
            0,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "b", Some(20)),
            400_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "c", Some(40)),
            800_000,
        ),
        // five seconds of nothing, same part still open
        (
            "item/agentMessage/delta",
            answer_delta(item, "d", Some(60)),
            5_800_000,
        ),
        (
            "item/completed",
            answer_completed(item, "final_answer", "abcd"),
            5_800_000,
        ),
    ]);
    let tps = measurements[0].tps.unwrap();
    assert_eq!(measurements[0].duration_ms, 5_800.0);
    assert!(
        tps < 20.0,
        "silence was excluded from the denominator: {tps}"
    );
}

#[test]
fn a_tool_call_ends_the_segment_and_output_after_it_starts_a_new_one() {
    let mut events = clean_segment("msg_pre", 0);
    events.pop(); // the tool arrives before this item ever completes
    events.push(("item/started", tool_started("exec-1"), 2_000_000));
    events.push((
        "item/completed",
        json!({"item": {"id": "exec-1", "type": "commandExecution"}}),
        20_000_000,
    ));
    events.extend(clean_segment("msg_post", 21_000_000));
    let measurements = replay(&events);

    assert_eq!(
        measurements.len(),
        2,
        "tool time merged two segments into one"
    );
    assert_eq!(measurements[0].key.item_id, "msg_pre");
    assert_eq!(measurements[1].key.item_id, "msg_post");
    // 18 s of tool execution sits between them and is in neither denominator.
    assert_eq!(measurements[0].duration_ms, 1_200.0);
    assert_eq!(measurements[1].duration_ms, 1_200.0);
    assert!((measurements[1].tps.unwrap() - 50.0).abs() < 1e-9);
}

#[test]
fn interleaved_generation_and_tool_calls_exclude_interruptions_and_tool_time() {
    // Generation → tool (18s) → generation → interrupted generation.
    // TPS must come from contiguous non-interrupted generation deltas only.
    let mut events = clean_segment("msg_pre", 0);
    events.pop();
    events.push(("item/started", tool_started("exec-1"), 2_000_000));
    events.push((
        "item/completed",
        json!({"item": {"id": "exec-1", "type": "commandExecution"}}),
        20_000_000,
    ));
    events.extend(clean_segment("msg_mid", 21_000_000));
    let mut cut = clean_segment("msg_cut", 30_000_000);
    cut.pop();
    cut.push(("turn/interrupted", json!({}), 31_300_000));
    events.extend(cut);

    let measurements = replay(&events);
    assert_eq!(
        measurements.len(),
        3,
        "tool time or the interrupt merged segments"
    );

    let pre = measurements
        .iter()
        .find(|measurement| measurement.key.item_id == "msg_pre")
        .expect("pre-tool generation segment");
    let mid = measurements
        .iter()
        .find(|measurement| measurement.key.item_id == "msg_mid")
        .expect("post-tool generation segment");
    let interrupted = measurements
        .iter()
        .find(|measurement| measurement.key.item_id == "msg_cut")
        .expect("interrupted generation segment");

    assert_eq!(pre.duration_ms, 1_200.0);
    assert_eq!(mid.duration_ms, 1_200.0);
    assert!((pre.tps.unwrap() - 50.0).abs() < 1e-9);
    assert!((mid.tps.unwrap() - 50.0).abs() < 1e-9);
    assert!(
        pre.duration_ms < 5_000.0 && mid.duration_ms < 5_000.0,
        "18s of tool execution leaked into generation TPS"
    );
    assert_eq!(interrupted.status, SegmentStatus::Partial);
    assert!(
        interrupted.duration_ms < 5_000.0,
        "tool time leaked into the interrupted segment: {}",
        interrupted.duration_ms
    );
    assert!(
        !interrupted.is_publishable(),
        "an interrupted segment must not become the headline rate"
    );
}

#[test]
fn two_model_responses_in_one_turn_never_share_a_numerator_or_a_denominator() {
    let mut events = clean_segment("msg_first", 0);
    events.extend(clean_segment("msg_second", 10_000_000));
    let measurements = replay(&events);
    assert_eq!(measurements.len(), 2);
    for measurement in &measurements {
        assert_eq!(measurement.exact_tokens_after_first_sample, 60);
        assert_eq!(measurement.duration_ms, 1_200.0);
        assert!((measurement.tps.unwrap() - 50.0).abs() < 1e-9);
    }
    assert_ne!(measurements[0].key.item_id, measurements[1].key.item_id);
    assert_ne!(
        measurements[0].measurement_id,
        measurements[1].measurement_id
    );
}

#[test]
fn reasoning_and_tool_argument_deltas_contribute_no_visible_answer_tokens() {
    let baseline = replay(&clean_segment("msg_only", 0));
    let mut noisy: Vec<(&str, Value, i64)> = vec![
        (
            "item/started",
            json!({"item": {"id": "rs_1", "type": "reasoning"}}),
            0,
        ),
        (
            "item/reasoning/textDelta",
            json!({"itemId": "rs_1", "delta": "thinking hard"}),
            0,
        ),
        (
            "item/completed",
            json!({"item": {"id": "rs_1", "type": "reasoning"}}),
            0,
        ),
    ];
    noisy.extend(clean_segment("msg_only", 0));
    noisy.push((
        "response.function_call_arguments.delta",
        json!({"item_id": "call_1", "delta": "{\"path\":\"/tmp\"}"}),
        2_000_000,
    ));
    let measurements = replay(&noisy);
    let answer = measurements
        .iter()
        .find(|measurement| measurement.key.item_id == "msg_only")
        .expect("the answer segment survives");
    assert_eq!(answer.tps, baseline[0].tps);
    assert_eq!(answer.exact_tokens_after_first_sample, 60);
    assert_eq!(answer.sample_count, baseline[0].sample_count);
}

#[test]
fn turn_level_usage_is_never_borrowed_as_segment_usage() {
    // The turn reported 322 output tokens across two answer segments and a
    // reasoning item. That number describes none of them, and each segment says
    // so by name rather than dividing by a denominator it does not own.
    let item = "msg_scope";
    let with_turn_usage = |text: &str, at: i64| {
        (
            "item/agentMessage/delta",
            json!({
                "delta": text,
                "itemId": item,
                "turnId": "turn-1",
                "tokenUsage": {"last": {"outputTokens": 322, "reasoningOutputTokens": 133}}
            }),
            at,
        )
    };
    let measurements = replay(&[
        with_turn_usage("a", 0),
        with_turn_usage("b", 400_000),
        with_turn_usage("c", 800_000),
        with_turn_usage("d", 1_200_000),
        (
            "item/completed",
            answer_completed(item, "final_answer", "abcd"),
            1_200_000,
        ),
    ]);
    assert_eq!(measurements[0].tps, None);
    assert_eq!(
        measurements[0].unavailable_reason,
        Some(UnavailableReason::UsageScopeMismatch)
    );
    assert_eq!(
        measurements[0].token_count_source,
        TokenCountSource::Unavailable
    );
}

#[test]
fn text_volume_never_becomes_a_token_count() {
    // A long answer with no exact token source is unavailable, not estimated.
    let item = "msg_prose";
    let long = "a".repeat(4_000);
    let measurements = replay(&[
        (
            "item/agentMessage/delta",
            answer_delta(item, &long, None),
            0,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, &long, None),
            400_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, &long, None),
            800_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, &long, None),
            1_200_000,
        ),
        (
            "item/completed",
            answer_completed(item, "final_answer", &long),
            1_200_000,
        ),
    ]);
    assert_eq!(measurements[0].tps, None);
    assert_eq!(
        measurements[0].unavailable_reason,
        Some(UnavailableReason::MissingExactTokenSource)
    );
    assert_eq!(measurements[0].exact_tokens_after_first_sample, 0);
}

#[test]
fn a_repeat_or_a_regression_in_the_protocol_sequence_disqualifies_the_segment() {
    let item = "msg_seq";
    let sequenced = |text: &str, tokens: i64, sequence: i64, at: i64| {
        (
            "response.output_text.delta",
            json!({
                "delta": text,
                "item_id": item,
                "output_index": 0,
                "content_index": 0,
                "sequence_number": sequence,
                "cumulative_output_tokens": tokens
            }),
            at,
        )
    };
    let done = (
        "response.output_text.done",
        json!({"item_id": item, "type": "message", "output_index": 0, "content_index": 0}),
        1_600_000,
    );

    let intact = replay(&[
        sequenced("a", 0, 1, 0),
        sequenced("b", 20, 2, 400_000),
        sequenced("c", 40, 3, 800_000),
        sequenced("d", 60, 4, 1_200_000),
        done.clone(),
    ]);
    assert!(intact[0].tps.is_some());
    assert!(intact[0].quality_flags.is_empty());

    // A skipped number is recorded but not fatal: samples carry cumulative
    // token counts, so a missing delta drops a point from the regression
    // without changing its slope. Many transports also number every event in
    // the response, which makes holes between one part's deltas routine.
    let gap = replay(&[
        sequenced("a", 0, 1, 0),
        sequenced("b", 20, 2, 400_000),
        sequenced("d", 60, 9, 1_200_000),
        sequenced("e", 80, 10, 1_600_000),
        done.clone(),
    ]);
    let gapped = gap[0].tps.expect("a hole does not disqualify the segment");
    assert!(gap[0]
        .quality_flags
        .contains(&QualityFlag::SequenceGapObserved));
    // Same slope as the intact series: 50 tok/s. The hole did not inflate it.
    assert!((gapped - 50.0).abs() < 1e-9, "{gapped}");

    // A repeat adds a later timestamp carrying no new tokens, which flattens
    // the slope, and a regression breaks the pairing outright. Neither is
    // recoverable, so the segment publishes nothing.
    for broken in [
        vec![
            sequenced("a", 0, 1, 0),
            sequenced("b", 20, 2, 400_000),
            sequenced("b", 20, 2, 800_000),
            sequenced("c", 40, 3, 1_200_000),
            done.clone(),
        ],
        vec![
            sequenced("a", 0, 1, 0),
            sequenced("b", 20, 3, 400_000),
            sequenced("c", 40, 2, 800_000),
            sequenced("d", 60, 4, 1_200_000),
            done.clone(),
        ],
    ] {
        let measurements = replay(&broken);
        assert_eq!(measurements[0].tps, None);
        assert_eq!(
            measurements[0].unavailable_reason,
            Some(UnavailableReason::SequenceGap)
        );
        assert!(measurements[0]
            .quality_flags
            .contains(&QualityFlag::OutOfOrderEvent));
    }
}
#[test]
fn batched_arrivals_are_flagged_and_do_not_count_as_distinct_samples() {
    let item = "msg_batch";
    let measurements = replay(&[
        (
            "item/agentMessage/delta",
            answer_delta(item, "a", Some(0)),
            0,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "b", Some(20)),
            900_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "c", Some(40)),
            900_000,
        ),
        (
            "item/completed",
            answer_completed(item, "final_answer", "abc"),
            900_000,
        ),
    ]);
    assert!(measurements[0]
        .quality_flags
        .contains(&QualityFlag::BatchedDelivery));
    assert_eq!(measurements[0].sample_count, 3);
    // Three arrivals but only two instants: not four distinct samples.
    assert_eq!(
        measurements[0].unavailable_reason,
        Some(UnavailableReason::InsufficientSamples)
    );
}

#[test]
fn an_interrupted_segment_is_partial_and_never_a_completed_headline() {
    let mut events = clean_segment("msg_cut", 0);
    events.pop(); // no item/completed: the stream was cut
    events.push(("turn/interrupted", json!({}), 1_300_000));
    let measurements = replay(&events);
    assert_eq!(measurements[0].status, SegmentStatus::Partial);
    assert!(
        measurements[0].tps.is_some(),
        "partial evidence is still evidence"
    );
    assert!(
        !measurements[0].is_publishable(),
        "a partial segment must stay out of headline and history"
    );
}

#[test]
fn replaying_one_frozen_fixture_produces_an_identical_measurement() {
    let mut events = clean_segment("msg_replay", 0);
    events.extend(clean_segment("msg_replay_2", 9_000_000));
    events.push(("turn/completed", json!({}), 11_000_000));
    assert_eq!(replay(&events), replay(&events));
}

#[test]
fn the_normalizer_maps_lifecycle_events_and_ignores_what_it_cannot_identify() {
    assert!(matches!(
        protocol_event("item/started", &answer_started("msg_x", "commentary")),
        Some(ProtocolEvent::TextItemStarted { .. })
    ));
    assert!(matches!(
        protocol_event(
            "item/completed",
            &answer_completed("msg_x", "final_answer", "hi")
        ),
        Some(ProtocolEvent::TextSegmentDone { .. })
    ));
    assert!(matches!(
        protocol_event("item/started", &tool_started("exec-9")),
        Some(ProtocolEvent::NonTextItem { .. })
    ));
    assert!(matches!(
        protocol_event("turn/failed", &json!({})),
        Some(ProtocolEvent::ResponseTerminal { interrupted: true })
    ));
    // An empty delta delivers nothing, and a delta without identity cannot be
    // attributed to a segment: neither may open or extend a measurement.
    assert!(protocol_event(
        "item/agentMessage/delta",
        &answer_delta("msg_x", "", Some(1))
    )
    .is_none());
    assert!(protocol_event("item/agentMessage/delta", &json!({"delta": "hi"})).is_none());
    assert!(protocol_event("thread/tokenUsage/updated", &json!({})).is_none());
}

/// A real captured turn — reasoning, an answer, shell and MCP tool calls, more
/// reasoning, and a second answer — replayed through the same normalizer the
/// pump uses. This is the agentic shape the turn-wide estimate handled worst.
///
/// Deltas are length-preserved placeholders and item payloads are trimmed to the
/// fields segmentation reads; nothing about the identity, ordering, or arrival
/// timing of the stream is altered.
const REAL_TURN_FIXTURE: &str = include_str!("fixtures/codex_turn_answer_tools_answer.json");

#[test]
fn a_captured_codex_turn_segments_into_its_own_answers_and_publishes_no_rate() {
    let events: Vec<Value> = serde_json::from_str(REAL_TURN_FIXTURE).unwrap();
    let mut tracker = TurnSegmentTracker::new(
        "sess",
        "01a00d56-6d99-78b0-86db-39cb7ba3492a",
        Some("synth-cloud".into()),
        None,
    );
    for event in &events {
        let method = event["method"].as_str().unwrap();
        let at_us = event["receivedAtUs"].as_i64().unwrap();
        if let Some(normalized) = protocol_event(method, &event["params"]) {
            tracker.observe(normalized, at_us);
        }
    }
    tracker.finish();
    let measurements = tracker.measurements();

    // The turn produced two answer items. Reasoning, the user message, and the
    // tool item are not answer segments and produce nothing.
    assert_eq!(measurements.len(), 2, "{measurements:#?}");
    assert!(measurements
        .iter()
        .all(|measurement| measurement.key.item_id.starts_with("msg_")));
    assert!(measurements
        .iter()
        .all(|measurement| measurement.phase == SegmentPhase::Commentary));
    // Eighteen seconds of shell and MCP execution separate them and sit in
    // neither denominator; a turn-wide figure would have spanned both answers.
    let between =
        measurements[1].samples[0].at_us - (measurements[0].samples.last().unwrap().at_us);
    assert!(
        between > 8_000_000,
        "tool time between segments: {between} us"
    );
    assert!(measurements
        .iter()
        .all(|measurement| measurement.duration_ms < 2_000.0));
    assert!(measurements
        .iter()
        .all(|measurement| measurement.sample_count > 4));

    // This provider reports no per-item token usage, so no rate is published —
    // the 545–643 tok/s figures the turn-wide estimate used to produce are gone,
    // and nothing stands in for the tokens that were never reported.
    for measurement in measurements {
        assert_eq!(measurement.tps, None);
        assert_eq!(measurement.status, SegmentStatus::Unavailable);
        assert_eq!(
            measurement.unavailable_reason,
            Some(UnavailableReason::MissingExactTokenSource)
        );
        assert_eq!(measurement.exact_tokens_after_first_sample, 0);
        // The evidence is still complete enough to audit the refusal.
        assert!(measurement.duration_ms > 0.0);
        assert_eq!(measurement.samples.len(), measurement.sample_count);
    }
}

/// The same fixture with the provider's exact per-item counts supplied: the
/// pipeline that refuses above publishes a rate the moment the tokens exist,
/// and it comes from that one segment's samples.
#[test]
fn the_same_captured_turn_publishes_a_rate_once_exact_tokens_are_reported() {
    let mut events: Vec<Value> = serde_json::from_str(REAL_TURN_FIXTURE).unwrap();
    let mut running: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for event in &mut events {
        if event["method"] != "item/agentMessage/delta" {
            continue;
        }
        let item = event["params"]["itemId"].as_str().unwrap().to_owned();
        let tokens = running.entry(item).or_insert(0);
        *tokens += 1;
        event["params"]["cumulativeOutputTokens"] = json!(*tokens);
    }
    let mut tracker = TurnSegmentTracker::new("sess", "turn", Some("synth-cloud".into()), None);
    for event in &events {
        if let Some(normalized) =
            protocol_event(event["method"].as_str().unwrap(), &event["params"])
        {
            tracker.observe(normalized, event["receivedAtUs"].as_i64().unwrap());
        }
    }
    tracker.finish();
    let measurements = tracker.measurements();
    assert_eq!(measurements.len(), 2);
    for measurement in measurements {
        let tps = measurement
            .tps
            .expect("exact tokens make the rate publishable");
        assert!(tps.is_finite() && tps > 0.0, "{tps}");
        assert_eq!(measurement.status, SegmentStatus::Completed);
        assert_eq!(
            measurement.token_count_source,
            TokenCountSource::ProviderItemUsage
        );
        // Tokens and time share one scope: the rate is inside the bounds the
        // segment's own first and last samples allow.
        let seconds = measurement.duration_ms / 1_000.0;
        let mean = measurement.exact_tokens_after_first_sample as f64 / seconds;
        assert!(
            tps > mean / 4.0 && tps < mean * 4.0,
            "regressed {tps} is not of the same order as the segment mean {mean}"
        );
    }
}

/// The wired path: pump events in, a persisted measurement out.
///
/// Exercises `track_performance_event` itself — the same call the stdout pump
/// makes — so the tracker, the token-authority checks, persistence, and the
/// turn ledger are all covered by one replay rather than trusted separately.
#[tokio::test]
async fn the_pump_path_persists_one_measurement_per_segment_with_its_samples() {
    let temp = tempdir().unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let persistence = SessionPersistence::from_core(Some(core.clone()));
    let receipts = credential_broker::ReceiptStore::new();
    let session = "gs-pump";
    let trackers: PerformanceTrackers = Arc::default();
    trackers.lock().await.insert(
        session.to_owned(),
        tracker_for(session, "synth-cloud", "turn-gs"),
    );

    let item = "msg_pump";
    let mut published = Vec::new();
    let mut events = clean_segment(item, 0);
    events.push(("turn/completed", json!({}), 2_000_000));
    for (method, params, at_us) in &events {
        published.extend(
            track_performance_event(
                &persistence,
                &trackers,
                &receipts,
                session,
                method,
                params,
                *at_us,
            )
            .await,
        );
    }
    assert_eq!(published.len(), 1);
    assert!((published[0].tps.unwrap() - 50.0).abs() < 1e-9);

    let stored = GenerationSpeedRepository::new(core.storage().database().clone())
        .for_turn(session.into(), "turn-gs".into())
        .await
        .unwrap();
    let [row] = stored.as_slice() else {
        panic!("expected one persisted measurement, got {}", stored.len());
    };
    assert_eq!(row.item_id, item);
    assert_eq!(row.status, "completed");
    assert_eq!(row.phase, "final_answer");
    assert_eq!(row.token_count_source, "provider_item_usage");
    assert_eq!(row.clock_source, "workshop_monotonic_receive");
    assert_eq!(row.unavailable_reason, None);
    assert_eq!(row.exact_tokens_after_first_sample, 60);

    // The evidence is stored, not just the scalar: the row recomputes itself.
    let samples: Vec<Value> = serde_json::from_str(&row.samples_json).unwrap();
    assert_eq!(samples.len(), 4);
    let points: Vec<(f64, f64)> = samples
        .iter()
        .map(|sample| {
            (
                sample["atUs"].as_f64().unwrap(),
                sample["cumulativeTokens"].as_f64().unwrap(),
            )
        })
        .collect();
    let recomputed = ols_tokens_per_second(&points).unwrap();
    assert!((recomputed - row.tps.unwrap()).abs() < 1e-9);

    // The turn's ledger row carries that same segment's rate, labelled as a
    // segment measurement — not a turn-wide ratio wearing the old name.
    let totals = usage_totals(&core).await;
    assert_eq!(totals.requests, 1);
    assert!((totals.decode_tps_p50.unwrap() - 50.0).abs() < 1e-9);
}

/// The same replay without exact tokens: nothing is published, the refusal is
/// recorded with its reason, and the ledger carries no rate at all.
#[tokio::test]
async fn a_segment_without_exact_tokens_records_its_refusal_and_leaves_the_ledger_rateless() {
    let temp = tempdir().unwrap();
    let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
    let persistence = SessionPersistence::from_core(Some(core.clone()));
    let receipts = credential_broker::ReceiptStore::new();
    let session = "gs-pump-refuses";
    let trackers: PerformanceTrackers = Arc::default();
    trackers.lock().await.insert(
        session.to_owned(),
        tracker_for(session, "synth-cloud", "turn-gs"),
    );

    let item = "msg_refuse";
    let events: Vec<(&str, Value, i64)> = vec![
        ("item/started", answer_started(item, "final_answer"), 0),
        ("item/agentMessage/delta", answer_delta(item, "a", None), 0),
        (
            "item/agentMessage/delta",
            answer_delta(item, "b", None),
            400_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "c", None),
            800_000,
        ),
        (
            "item/agentMessage/delta",
            answer_delta(item, "d", None),
            1_200_000,
        ),
        (
            "item/completed",
            answer_completed(item, "final_answer", "abcd"),
            1_200_000,
        ),
        ("turn/completed", json!({}), 2_000_000),
    ];
    for (method, params, at_us) in &events {
        track_performance_event(
            &persistence,
            &trackers,
            &receipts,
            session,
            method,
            params,
            *at_us,
        )
        .await;
    }

    let stored = GenerationSpeedRepository::new(core.storage().database().clone())
        .for_turn(session.into(), "turn-gs".into())
        .await
        .unwrap();
    let [row] = stored.as_slice() else {
        panic!("expected one persisted measurement, got {}", stored.len());
    };
    assert_eq!(row.tps, None);
    assert_eq!(row.status, "unavailable");
    assert_eq!(
        row.unavailable_reason.as_deref(),
        Some("missing_exact_token_source")
    );
    assert_eq!(row.sample_count, 4, "the timing evidence is still kept");

    // The old formula would have divided 200 turn-level output tokens by the
    // 1.2 s it kept and published ~167 tok/s here.
    let totals = usage_totals(&core).await;
    assert_eq!(totals.requests, 1);
    assert_eq!(totals.decode_tps_p50, None);
    assert!(
        totals.end_to_end_tps_p50.is_some(),
        "latency is still tracked"
    );
}
