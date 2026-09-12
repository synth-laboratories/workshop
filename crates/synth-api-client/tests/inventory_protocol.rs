use synth_api_client::{
    inventory::{Disposition, RuntimeInventory},
    RuntimeKind,
};

#[tokio::test]
async fn inventory_read_encodes_recorded_identity_for_both_kinds() {
    use synth_api_client::InternClient;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for (kind, segment) in [("sync", "sync-sessions"), ("async", "async-assignments")] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            let size = socket.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..size]).to_lowercase();
            assert!(request.starts_with(&format!(
                "get /api/v1/smr/research-intern/{segment}/recorded%2fid/resources "
            )));
            assert!(request.contains("cache-control: no-store"));
            let body = serde_json::json!({"runtime_kind":kind,"runtime_id":"recorded/id",
                "observed_at":"2026-09-12T00:00:00Z","coverage":"registered-runtime-resources-v1",
                "coverage_complete":false,"incomplete_reasons":["unqualified"],"resources":[]})
            .to_string();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
    });
    let client = InternClient::connect(
        &format!("http://{address}/api/v1"),
        "fixture",
        std::time::Duration::from_secs(2),
    )
    .unwrap();
    for kind in [RuntimeKind::Sync, RuntimeKind::Async] {
        assert!(
            !client
                .runtime_resources(kind, "recorded/id")
                .await
                .unwrap()
                .coverage_complete
        );
    }
    server.await.unwrap();
}

#[test]
fn inventory_fields_and_dispositions_match_pinned_backend_schema() {
    use std::collections::BTreeSet;
    use synth_api_client::inventory::{InventoryCoverage, Relation, ResourceDisposition};
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../contracts/research-v1.json")).unwrap();
    let models = &schema["components"]["schemas"];
    let item = ResourceDisposition {
        resource_kind: "intern_runtime".into(),
        resource_id: "runtime".into(),
        cleanup_owner_run_id: None,
        relation: Relation::Self_,
        disposition: Disposition::Unknown,
        reason: "unconfirmed".into(),
    };
    let inventory = RuntimeInventory {
        runtime_kind: RuntimeKind::Async,
        runtime_id: "runtime".into(),
        observed_at: "2026-09-12T00:00:00Z".into(),
        coverage: InventoryCoverage::RegisteredRuntimeResourcesV1,
        coverage_complete: false,
        incomplete_reasons: vec!["unconfirmed".into()],
        resources: vec![item.clone()],
    };
    for (name, value) in [
        (
            "InternResourceDisposition",
            serde_json::to_value(item).unwrap(),
        ),
        (
            "InternResourceInventory",
            serde_json::to_value(inventory).unwrap(),
        ),
    ] {
        let actual: BTreeSet<_> = value.as_object().unwrap().keys().collect();
        let expected: BTreeSet<_> = models[name]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(actual, expected, "field drift for {name}");
    }
    for value in models["InternResourceDisposition"]["properties"]["disposition"]["enum"]
        .as_array()
        .unwrap()
    {
        let parsed: Disposition = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), *value);
    }
    for value in models["InternResourceDisposition"]["properties"]["relation"]["enum"]
        .as_array()
        .unwrap()
    {
        let parsed: Relation = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), *value);
    }
}

#[test]
fn incomplete_inventory_preserves_unknown_and_rejects_drift() {
    let value = serde_json::json!({"runtime_kind":"async", "runtime_id":"recorded",
        "observed_at":"2026-09-12T00:00:00Z", "coverage":"registered-runtime-resources-v1",
        "coverage_complete":false, "incomplete_reasons":["host_unconfirmed"],
        "resources":[{"resource_kind":"intern_runtime", "resource_id":"recorded",
        "relation":"self", "disposition":"unknown", "reason":"host_unconfirmed"}]});
    let inventory: RuntimeInventory = serde_json::from_value(value.clone()).unwrap();
    inventory
        .validate_identity(RuntimeKind::Async, "recorded")
        .unwrap();
    assert_eq!(inventory.resources[0].disposition, Disposition::Unknown);
    assert!(inventory
        .validate_identity(RuntimeKind::Async, "other")
        .is_err());
    let mut invalid = value;
    invalid["coverage_complete"] = true.into();
    assert!(serde_json::from_value::<RuntimeInventory>(invalid.clone())
        .unwrap()
        .validate_identity(RuntimeKind::Async, "recorded")
        .is_err());
    invalid["resources"][0]["provider_handle"] = "private".into();
    assert!(serde_json::from_value::<RuntimeInventory>(invalid).is_err());
}
