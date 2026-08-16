//! Container capability gating against two real loopback services.
//!
//! The raw engine is healthy, advertises SSE, and would happily count a
//! `POST /rollouts/prepare`. The normalized pool advertises the live-eval
//! protocol. Gating is only real if the raw engine's prepare counter stays at
//! zero while the normalized pool receives exactly one prepare.

use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use synth_desktop_lib::container_capabilities::{
    ContainerPreflightError, CODE_CAPABILITY_MISMATCH, LIVE_EVAL_PROTOCOL,
};
use synth_desktop_lib::core_runtime::CoreRuntime;
use synth_desktop_lib::visuals_ipc::dispatch;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct FakeService {
    addr: SocketAddr,
    prepares: Arc<AtomicUsize>,
}

impl FakeService {
    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn prepare_count(&self) -> usize {
        self.prepares.load(Ordering::SeqCst)
    }
}

fn healthy() -> Value {
    json!({"ok": true, "status": "ok"})
}

async fn spawn_service(info: Value, supports_prepare: bool) -> FakeService {
    spawn_service_with_health(info, supports_prepare, healthy()).await
}

async fn spawn_service_with_health(
    info: Value,
    supports_prepare: bool,
    health: Value,
) -> FakeService {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let prepares = Arc::new(AtomicUsize::new(0));
    let counter = prepares.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let info = info.clone();
            let health = health.clone();
            let counter = counter.clone();
            tokio::spawn(async move {
                let _ = serve_once(stream, info, health, supports_prepare, counter).await;
            });
        }
    });
    FakeService { addr, prepares }
}

async fn serve_once(
    mut stream: TcpStream,
    info: Value,
    health: Value,
    supports_prepare: bool,
    prepares: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 2048];
    let (head, body) = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        raw.extend_from_slice(&buffer[..read]);
        let text = String::from_utf8_lossy(&raw).to_string();
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            continue;
        };
        let expected = content_length(head);
        if body.len() >= expected {
            break (head.to_string(), body.to_string());
        }
    };
    let request_line = head.lines().next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();

    let (status, payload) = match (method, path) {
        // Always HTTP 200: the point is that the payload, not the status
        // code, decides readiness.
        ("GET", "/health") => (200, health),
        ("GET", "/info") => (200, info),
        ("POST", "/rollouts/prepare") if supports_prepare => {
            prepares.fetch_add(1, Ordering::SeqCst);
            let rollout_id = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("rollout_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "roll_unknown".into());
            (
                200,
                json!({
                    "rollout_id": rollout_id,
                    "status": "prepared",
                    "stream": {
                        "id": format!("stream_{rollout_id}"),
                        "cursor": {"kind": "sequence"},
                        "transports": {
                            "poll": {"url": format!("/rollouts/{rollout_id}/poll")},
                            "sse": {"url": format!("/rollouts/{rollout_id}/stream/sse")}
                        }
                    }
                }),
            )
        }
        ("POST", "/rollouts/prepare") => {
            // A raw engine still counts the attempt: the assertion is that
            // Workshop never reaches this arm.
            prepares.fetch_add(1, Ordering::SeqCst);
            (405, json!({"detail": "method not allowed"}))
        }
        _ => (404, json!({"detail": "not found"})),
    };

    let encoded = serde_json::to_vec(&payload).unwrap();
    let response = format!(
        "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        encoded.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&encoded).await?;
    stream.flush().await
}

fn content_length(head: &str) -> usize {
    head.lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

fn raw_engine_info() -> Value {
    json!({
        "env_family": "craftax",
        "service": "craftax-rust-gold",
        "rollout_stream_sse": true,
        "interactive_rollout": true,
        "features": ["rollout_stream_sse", "interactive_rollout"],
        "capabilities": {"async_rollout": true, "checkpoint_resume": true}
    })
}

fn normalized_pool_info() -> Value {
    json!({
        "env_family": "craftax",
        "service": {"task": "gamebench-craftax-rust-react"},
        "capabilities": {
            "protocol": LIVE_EVAL_PROTOCOL,
            "operations": {
                "rollouts.prepare": true,
                "rollouts.start_prepared": true,
                "rollouts.get": true,
                "rollouts.poll": true,
                "reward.get": true,
                "trace_v5.capture": false
            },
            "policy_refs": [
                {"harness": "react", "config": "luna_low", "model": "gpt-5.6-luna"}
            ]
        }
    })
}

async fn register(core: &CoreRuntime, base_url: &str, name: &str) -> Value {
    dispatch(
        "POST",
        "/v1/containers",
        json!({"baseUrl": base_url, "name": name, "location": "local"}),
        core,
    )
    .await
    .expect("register")
}

fn preflight_error(error: &anyhow::Error) -> &ContainerPreflightError {
    error
        .downcast_ref::<ContainerPreflightError>()
        .unwrap_or_else(|| panic!("expected a structured preflight failure, got: {error:#}"))
}

#[tokio::test]
async fn container_raw_engine_is_rejected_before_any_prepare_request() {
    let raw = spawn_service(raw_engine_info(), true).await;
    let dir = tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();

    let registered = register(&core, &raw.base_url(), "raw-gold").await;
    let container_id = registered["container"]["id"].as_str().unwrap().to_string();
    assert_eq!(registered["container"]["status"], "ready");
    let capabilities = &registered["container"]["metadata"]["capabilities"];
    assert_eq!(capabilities["source"], "none");
    assert_eq!(capabilities["operations"]["rollouts.prepare"], "unknown");
    assert_eq!(capabilities["complete"], false);

    let failure = dispatch(
        "POST",
        &format!("/v1/containers/{container_id}/rollouts/prepare"),
        json!({"rollout_id": "roll_raw_1"}),
        &core,
    )
    .await
    .expect_err("raw engine must not prepare");
    let error = preflight_error(&failure);
    assert_eq!(error.code, CODE_CAPABILITY_MISMATCH);
    assert!(!error.retryable);
    assert!(error.missing.contains(&"rollouts.prepare".to_string()));

    assert_eq!(
        raw.prepare_count(),
        0,
        "capability gating must reject before the network call"
    );

    // No registration, no port guessing, no fallback selection, no rollout.
    let listed = dispatch("GET", "/v1/containers", json!({}), &core)
        .await
        .expect("list");
    let containers = listed["containers"].as_array().unwrap();
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0]["id"], container_id.as_str());
    assert!(containers[0]["lastRolloutId"].is_null());
}

#[tokio::test]
async fn container_unhealthy_pool_is_rejected_before_any_prepare_request() {
    let pool = spawn_service(normalized_pool_info(), true).await;
    let dir = tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();
    let registered = register(&core, &pool.base_url(), "luna-pool").await;
    let container_id = registered["container"]["id"].as_str().unwrap().to_string();

    // The pool dies after registration, exactly like the observed 8104 record.
    core.update_container_health(
        container_id.clone(),
        "unhealthy".into(),
        json!({"ok": false, "error": "connection refused"}),
    )
    .await
    .unwrap();

    let failure = dispatch(
        "POST",
        &format!("/v1/containers/{container_id}/rollouts/prepare"),
        json!({"rollout_id": "roll_dead_1"}),
        &core,
    )
    .await
    .expect_err("dead pool must not prepare");
    let error = preflight_error(&failure);
    assert_eq!(error.code, "container_unhealthy");
    assert!(error.retryable);
    assert_eq!(
        error.last_probe_error.as_deref(),
        Some("connection refused")
    );
    assert_eq!(pool.prepare_count(), 0);
}

/// HTTP 200 is not readiness. A pool that answers `200 {"ok": false}` must be
/// recorded unhealthy by **both** writers — registration and probe — or it
/// passes the health half of preflight and reaches a mutating call.
#[tokio::test]
async fn container_unhealthy_payload_under_http_200_is_never_ready() {
    for payload in [json!({"ok": false}), json!({"healthy": false})] {
        let pool = spawn_service_with_health(normalized_pool_info(), true, payload.clone()).await;
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();

        let registered = register(&core, &pool.base_url(), "sick-pool").await;
        let container_id = registered["container"]["id"].as_str().unwrap().to_string();
        assert_eq!(
            registered["container"]["status"], "unhealthy",
            "registration accepted {payload} as ready"
        );
        assert_eq!(registered["container"]["health"]["ok"], false);
        assert_eq!(registered["container"]["health"]["status"], 200);

        let probed = dispatch(
            "POST",
            &format!("/v1/containers/{container_id}/probe"),
            json!({}),
            &core,
        )
        .await
        .expect("probe");
        assert_eq!(
            probed["container"]["status"], "unhealthy",
            "probe accepted {payload} as ready"
        );
        assert_eq!(probed["container"]["health"]["ok"], false);

        // Capabilities are still projected — the pool advertises them — but
        // health closes the door first.
        assert_eq!(
            probed["container"]["metadata"]["capabilities"]["operations"]["rollouts.prepare"],
            "supported"
        );
        let failure = dispatch(
            "POST",
            &format!("/v1/containers/{container_id}/rollouts/prepare"),
            json!({"rollout_id": "roll_sick_1"}),
            &core,
        )
        .await
        .expect_err("unhealthy payload must not prepare");
        let error = preflight_error(&failure);
        assert_eq!(error.code, "container_unhealthy");
        assert!(error.retryable);
        assert_eq!(pool.prepare_count(), 0);
    }
}

/// `container_register.metadata` is agent-reachable through MCP. A caller
/// claiming the full protocol on a raw engine must not get past the gate.
#[tokio::test]
async fn container_caller_asserted_capabilities_do_not_unlock_a_raw_engine() {
    let raw = spawn_service(raw_engine_info(), true).await;
    let dir = tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();

    let asserted = json!({
        "protocol": LIVE_EVAL_PROTOCOL,
        "operations": {
            "rollouts.prepare": true,
            "rollouts.start_prepared": true,
            "rollouts.get": true,
            "rollouts.poll": true,
            "reward.get": true,
            "trace_v5.capture": true
        },
        "policy_refs": [{"harness": "react", "config": "luna_low"}]
    });
    let registered = dispatch(
        "POST",
        "/v1/containers",
        json!({
            "baseUrl": raw.base_url(),
            "name": "raw-gold-claiming-everything",
            "location": "local",
            "metadata": {
                "declaredCapabilities": asserted,
                "capabilities": asserted,
                "note": "unrelated caller metadata survives"
            }
        }),
        &core,
    )
    .await
    .expect("register");
    let container_id = registered["container"]["id"].as_str().unwrap().to_string();
    let metadata = &registered["container"]["metadata"];
    assert!(metadata.get("declaredCapabilities").is_none());
    assert_eq!(metadata["note"], "unrelated caller metadata survives");
    assert_eq!(metadata["capabilities"]["source"], "none");
    assert_eq!(
        metadata["capabilities"]["operations"]["rollouts.prepare"],
        "unknown"
    );

    let failure = dispatch(
        "POST",
        &format!("/v1/containers/{container_id}/rollouts/prepare"),
        json!({
            "rollout_id": "roll_claimed_1",
            "policy_ref": {"harness": "react", "config": "luna_low"}
        }),
        &core,
    )
    .await
    .expect_err("a self-asserted capability must not unlock prepare");
    assert_eq!(preflight_error(&failure).code, CODE_CAPABILITY_MISMATCH);
    assert_eq!(raw.prepare_count(), 0);
}

#[tokio::test]
async fn container_normalized_pool_prepares_exactly_once_after_a_fresh_probe() {
    let pool = spawn_service(normalized_pool_info(), true).await;
    let dir = tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();

    let registered = register(&core, &pool.base_url(), "luna-pool").await;
    let container_id = registered["container"]["id"].as_str().unwrap().to_string();

    let probed = dispatch(
        "POST",
        &format!("/v1/containers/{container_id}/probe"),
        json!({}),
        &core,
    )
    .await
    .expect("probe");
    let capabilities = &probed["container"]["metadata"]["capabilities"];
    assert_eq!(capabilities["source"], "info");
    assert_eq!(capabilities["protocol"], LIVE_EVAL_PROTOCOL);
    assert_eq!(capabilities["operations"]["rollouts.prepare"], "supported");
    assert_eq!(
        capabilities["operations"]["trace_v5.capture"],
        "unsupported"
    );
    assert_eq!(capabilities["policy_refs"][0]["config"], "luna_low");
    assert_eq!(capabilities["complete"], true);
    assert_eq!(
        pool.prepare_count(),
        0,
        "probe must not mutate the remote workload"
    );

    let prepared = dispatch(
        "POST",
        &format!("/v1/containers/{container_id}/rollouts/prepare"),
        json!({
            "rollout_id": "roll_pool_1",
            "policy_ref": {"harness": "react", "config": "luna_low"}
        }),
        &core,
    )
    .await
    .expect("normalized pool prepares");
    assert_eq!(prepared["rollout_id"], "roll_pool_1");
    assert_eq!(prepared["start_blocked_until"], "stream.subscribed");
    assert_eq!(pool.prepare_count(), 1);

    // A trace-sealing workflow still fails closed: this pool says
    // trace_v5.capture is unsupported, and no second prepare is sent.
    let failure = dispatch(
        "POST",
        &format!("/v1/containers/{container_id}/rollouts/prepare"),
        json!({
            "rollout_id": "roll_pool_2",
            "policy_ref": {"harness": "react", "config": "luna_low"},
            "require_trace_v5": true
        }),
        &core,
    )
    .await
    .expect_err("sealed-evidence workflow must not prepare");
    let error = preflight_error(&failure);
    assert_eq!(error.code, CODE_CAPABILITY_MISMATCH);
    assert_eq!(error.missing, vec!["trace_v5.capture".to_string()]);
    assert_eq!(pool.prepare_count(), 1);

    // A policy the pool does not advertise fails with the available refs and
    // no fallback to the advertised one.
    let failure = dispatch(
        "POST",
        &format!("/v1/containers/{container_id}/rollouts/prepare"),
        json!({
            "rollout_id": "roll_pool_3",
            "policy_ref": {"harness": "react", "config": "luna_med"}
        }),
        &core,
    )
    .await
    .expect_err("unadvertised policy must not prepare");
    let error = preflight_error(&failure);
    assert_eq!(error.code, CODE_CAPABILITY_MISMATCH);
    assert_eq!(error.available_policy_refs.len(), 1);
    assert_eq!(
        error.available_policy_refs[0].config.as_deref(),
        Some("luna_low")
    );
    assert_eq!(pool.prepare_count(), 1);
}
