use std::time::Duration;
use synth_api_client::{settlement::ResourceSettlement, InternClient, InternClientError};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn examples() -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(include_str!("fixtures/run_resource_settlement.json"))
        .unwrap()["examples"]
        .as_array()
        .unwrap()
        .clone()
}
#[test]
fn supplied_fixtures_preserve_incomplete_coverage_and_nullable_counts() {
    for value in examples() {
        let observation: ResourceSettlement = serde_json::from_value(value).unwrap();
        observation.validate_for_run(&observation.run_id).unwrap();
        assert!(!observation.reports_settled_root());
    }
    let mut value = examples()[1].clone();
    value["settled"] = true.into();
    let document: ResourceSettlement = serde_json::from_value(value.clone()).unwrap();
    assert!(document.validate_for_run("root-run").is_err());
    value["coverage_complete"] = true.into();
    let document: ResourceSettlement = serde_json::from_value(value.clone()).unwrap();
    assert!(document.reports_settled_root());
    value["scope_kind"] = "owned_subtree".into();
    value["edge_id"] = "child-edge".into();
    value["run_id"] = "child-run".into();
    let document: ResourceSettlement = serde_json::from_value(value.clone()).unwrap();
    document.validate_for_run("child-run").unwrap();
    assert!(!document.reports_settled_root());
    value["pending"] = (-1).into();
    assert!(serde_json::from_value::<ResourceSettlement>(value).is_err());
}
#[tokio::test]
async fn fresh_settlement_reads_encode_identity_and_never_reuse_stop_snapshots() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for observed in ["2026-09-11T12:00:00Z", "2026-09-11T12:00:01Z"] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            let count = socket.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..count]).to_lowercase();
            assert!(request.starts_with("get /api/v1/smr/runs/run%2fone%3fx/resource-settlement "));
            assert!(request.contains("cache-control: no-store"));
            assert!(request.contains("authorization: bearer fixture-only"));
            let body=serde_json::json!({"run_id":"run/one?x","observed_at":observed,"coverage":"untracked","settled":false}).to_string();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        }
    });
    let client = InternClient::connect(
        &format!("http://{address}/api/v1"),
        "fixture-only",
        Duration::from_secs(2),
    )
    .unwrap();
    let first = client.resource_settlement("run/one?x").await.unwrap();
    let second = client.resource_settlement("run/one?x").await.unwrap();
    assert_ne!(first.observed_at, second.observed_at);
    assert_eq!(first.pending, None);
    assert!(!second.reports_settled_root());
    server.await.unwrap();
}
#[tokio::test]
async fn evidence_unavailable_and_wrong_run_never_become_untracked_success() {
    for (status, headers, body) in [
        (
            503,
            "Cache-Control: no-store\r\n",
            "private database detail".to_owned(),
        ),
        (200, "", examples()[0].to_string()),
        (
            200,
            "Cache-Control: no-store\r\n",
            examples()[0].to_string(),
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            socket.read(&mut bytes).await.unwrap();
            socket.write_all(format!("HTTP/1.1 {status} Fixture\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        });
        let client = InternClient::connect(
            &format!("http://{address}"),
            "fixture-only",
            Duration::from_secs(2),
        )
        .unwrap();
        let error = client
            .resource_settlement("requested-run")
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("private database detail"));
        if status == 503 {
            assert!(matches!(error, InternClientError::Http { .. }));
        }
        server.await.unwrap();
    }
}
