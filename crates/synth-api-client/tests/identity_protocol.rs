use std::time::Duration;
use synth_api_client::{InternClient, InternClientError};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn document(account: &str) -> String {
    serde_json::json!({"schema_version":"synth.desktop-cloud-identity.v1","backend_origin":"https://fixture.invalid","backend_id":"00000000-0000-4000-8000-000000000001","profile_id":"00000000-0000-4000-8000-000000000002","account_id":account,"org_id":"00000000-0000-4000-8000-000000000004","verified_at":"2026-09-11T12:00:00Z","valid_until":"2026-09-11T12:01:00Z","credential_expiry":null,"revalidate_before_remote_operation":true,"revocation_contract":"fresh_database_key_and_membership_check"}).to_string()
}

#[tokio::test]
async fn identity_is_fetched_each_time_from_exact_candidate_route() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for account in ["first", "second"] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..count]).to_lowercase();
            assert!(request.starts_with("get /api/v1/desktop/cloud-identity "));
            assert!(request.contains("authorization: bearer fixture-only"));
            assert!(request.contains("cache-control: no-store"));
            let body = document(account);
            socket.write_all(format!("HTTP/1.1 200 OK\r\nCache-Control: private, no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        }
    });
    let client = InternClient::connect(
        &format!("http://{address}/api/v1"),
        "fixture-only",
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        client.identity_observation().await.unwrap().account_id,
        "first"
    );
    assert_eq!(
        client.identity_observation().await.unwrap().account_id,
        "second"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn identity_rejects_cached_oversized_and_denied_responses() {
    for (status, headers, body, auth) in [
        (200, "", document("a"), false),
        (200, "Cache-Control: no-store\r\n", "x".repeat(16385), false),
        (
            401,
            "Cache-Control: no-store\r\n",
            "sensitive provider detail".into(),
            true,
        ),
        (
            503,
            "Cache-Control: no-store\r\n",
            "database unavailable".into(),
            false,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            let response=format!("HTTP/1.1 {status} Fixture\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            let _ = socket.write_all(response.as_bytes()).await;
        });
        let client = InternClient::connect(
            &format!("http://{address}"),
            "fixture-only",
            Duration::from_secs(2),
        )
        .unwrap();
        let error = client.identity_observation().await.unwrap_err();
        assert_eq!(error.is_unauthenticated(), auth);
        assert!(!error.to_string().contains("sensitive provider detail"));
        if status == 503 {
            assert!(matches!(error, InternClientError::Http { .. }));
        }
        server.await.unwrap();
    }
}
