// Link the library crate rather than `#[path]`-including its sources: the
// re-included modules referenced `crate::domain`/`http`/`visuals`, which do not
// exist in a test crate, so the whole Rust suite stopped compiling.
use synth_desktop_lib::cloud::intern::{
    normalize_event, AsyncCommandRequest, AsyncEnsureRequest, CommandReceipt, InternClient,
    InternEvent, InternRuntime, RuntimeBinding, RuntimeKind, SyncCommandRequest, SyncCreateRequest,
};
use serde_json::{json, Map};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[test]
fn protocol_fixtures_parse_and_normalize() {
    let event: InternEvent =
        serde_json::from_str(include_str!("../src/cloud/intern/fixtures/sync_event.json")).unwrap();
    event.validate().unwrap();
    let normalized = normalize_event(event);
    assert_eq!(normalized.source, "intern");
    assert_eq!(normalized.remote_sequence, 1);
    assert_eq!(normalized.payload["intern"]["runtimeId"], "sync-1");

    let receipt: CommandReceipt = serde_json::from_str(include_str!(
        "../src/cloud/intern/fixtures/async_receipt.json"
    ))
    .unwrap();
    receipt.validate_for("cmd-async-1").unwrap();
    assert_eq!(receipt.runtime_kind, RuntimeKind::Async);
}

#[test]
fn command_receipt_semantics_distinguish_success_from_rejection() {
    let receipt: CommandReceipt = serde_json::from_str(include_str!(
        "../src/cloud/intern/fixtures/async_receipt.json"
    ))
    .unwrap();
    for status in ["received", "delivered", "applied", "noop"] {
        let mut candidate = receipt.clone();
        candidate.status = status.into();
        assert_eq!(candidate.local_terminal_status().unwrap(), "completed");
        candidate.validate_for("cmd-async-1").unwrap();
    }
    for status in ["refused", "superseded", "conflict"] {
        let mut candidate = receipt.clone();
        candidate.status = status.into();
        assert_eq!(candidate.local_terminal_status().unwrap(), "rejected");
        candidate.validate_for("cmd-async-1").unwrap();
    }
    let mut unknown = receipt;
    unknown.status = "unexpected".into();
    assert!(unknown.validate_for("cmd-async-1").is_err());
}

#[test]
fn desktop_command_constructors_match_wire_contract() {
    let sync = SyncCommandRequest::operator_message("cmd-1", "idem-1", 3, "Continue");
    assert_eq!(
        serde_json::to_value(sync).unwrap(),
        json!({
            "command_id": "cmd-1",
            "idempotency_key": "idem-1",
            "expected_generation": 3,
            "command_kind": "operator_message",
            "payload": {"body":"Continue", "context":{}, "turn_id":"cmd-1"},
            "execution_mode": "standard",
            "mode": "sync",
            "evidence_refs": []
        })
    );

    let async_message =
        AsyncCommandRequest::message("cmd-2", "idem-2", 7, "Investigate", Map::new());
    assert_eq!(
        serde_json::to_value(async_message).unwrap()["command_kind"],
        "message"
    );
}

#[test]
fn staging_create_contract_requires_nonempty_objectives() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../src/cloud/intern/fixtures/create_requests.json"
    ))
    .unwrap();
    let sync: SyncCreateRequest = serde_json::from_value(fixture["sync"].clone()).unwrap();
    let asynchronous: AsyncEnsureRequest =
        serde_json::from_value(fixture["async"].clone()).unwrap();
    sync.validate().unwrap();
    asynchronous.validate().unwrap();
    assert!(!sync.objective.trim().is_empty());
    assert!(!asynchronous.objective.trim().is_empty());

    assert!(
        SyncCreateRequest::desktop("", "sync-idem", RuntimeBinding::default())
            .validate()
            .is_err()
    );
    assert!(
        AsyncEnsureRequest::desktop("  ", "async-idem", RuntimeBinding::default())
            .validate()
            .is_err()
    );
}

#[tokio::test]
async fn bearer_client_posts_typed_sync_command_and_checks_identity() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let captured = Arc::new(tokio::sync::Mutex::new(String::new()));
    let server_capture = Arc::clone(&captured);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0_u8; 16_384];
        let count = socket.read(&mut request).await.unwrap();
        *server_capture.lock().await = String::from_utf8_lossy(&request[..count]).into_owned();
        let body = serde_json::to_string(&json!({
            "schema_version":"smr.intern-runtime-command-receipt.v1",
            "command_id":"cmd-1", "runtime_kind":"sync", "runtime_id":"sync-1",
            "status":"applied", "previous_generation":3, "state_generation":4,
            "decision_code":"applied", "created_at":"2026-08-08T00:00:00Z", "duplicate":false
        }))
        .unwrap();
        let response = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body);
        socket.write_all(response.as_bytes()).await.unwrap();
    });

    let client = InternClient::connect(
        &format!("http://{address}"),
        "secret-test-key",
        Duration::from_secs(2),
    )
    .unwrap();
    let request = SyncCommandRequest::operator_message("cmd-1", "idem-1", 3, "Continue");
    let receipt = client.command_sync("sync-1", &request).await.unwrap();
    assert_eq!(receipt.state_generation, 4);
    server.await.unwrap();
    let raw = captured.lock().await.to_lowercase();
    assert!(raw.starts_with("post /smr/research-intern/sync-sessions/sync-1/commands"));
    assert!(raw.contains("authorization: bearer secret-test-key"));
}

#[tokio::test]
async fn client_rejects_empty_credentials_and_unsafe_runtime_ids() {
    assert!(InternClient::connect("https://example.invalid", "", Duration::from_secs(1)).is_err());
    let client =
        InternClient::connect("https://example.invalid", "secret", Duration::from_secs(1)).unwrap();
    assert!(client.get_sync("../unsafe").await.is_err());
    let binding = RuntimeBinding::default();
    assert!(binding.factory_id.is_none());
}

#[tokio::test]
async fn runtime_is_fail_closed_when_unconfigured() {
    let runtime = InternRuntime::unconfigured();
    assert!(runtime.client().await.is_err());
    runtime.disable().await;
}
