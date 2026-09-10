use super::*;
use crate::storage::{ContentStore, persist_live_envelopes};

fn binding() -> Value { json!({"slots":[{"input":"stream","kind":"live_sse","source":"http://127.0.0.1/stream","poll_url":"http://127.0.0.1/events"}]}) }
fn events() -> Vec<Value> { vec![json!({"rollout_id":"a","event_id":"1","kind":"frame"}), json!({"rollout_id":"b","event_id":"1","kind":"verifier","payload":{"reward.txt":0.75}})] }

#[test]
fn host_observation_freezes_without_caller_snapshot_and_preserves_lanes() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::new(dir.path());
    let id = uuid::Uuid::new_v4().to_string();
    let input = binding();
    let declared = crate::visuals::stream_receipt::declared_streams(&input);
    crate::visuals::stream_receipt::record_poll_page(&id, 1, &declared, "http://127.0.0.1/events", &json!(events()));
    let (frozen, views) = freeze_fixture(input, &store, &id, 1).unwrap();
    let slot = &frozen["inputs"][0];
    assert_eq!(slot["kind"], "inline");
    assert!(slot.get("source").is_none());
    assert_eq!(slot["data"]["events"].as_array().unwrap().len(), 2);
    assert_eq!(slot["evidence"]["origin"], "host_observation");
    assert_eq!(views[0]["data"]["event_count"], 2);
    assert_eq!(views[0]["data"]["reward"], 0.75);
    assert!(views[0]["data"]["usage"].is_null());
    assert!(!serde_json::to_string(&frozen).unwrap().contains("127.0.0.1"));
    assert!(freeze_fixture(binding(), &store, &id, 2).is_err());
}

#[test]
fn spool_freezes_after_host_observation_is_gone_and_conflicts_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::new(dir.path());
    let spool = persist_live_envelopes(&store, Some("http://127.0.0.1/stream"), None, events()).unwrap();
    let mut input = binding();
    input["slots"][0]["spool_digest"] = json!(spool.digest);
    let (frozen, views) = freeze_fixture(input, &store, "never-observed", 1).unwrap();
    assert_eq!(frozen["inputs"][0]["evidence"]["origin"], "spool");
    assert_eq!(views[0]["data"]["event_count"], 2);
    let bad = vec![json!({"rollout_id":"a","event_id":"1","payload":1}), json!({"rollout_id":"a","event_id":"1","payload":2})];
    assert!(persist_live_envelopes(&store, None, None, bad).is_err());
}

#[test]
fn producer_binding_shaped_payload_is_not_recursively_frozen() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::new(dir.path());
    let mut input = binding();
    input["slots"][0]["snapshot"] = json!({"events":[{"kind":"frame","payload":{"kind":"live_sse","source":"quoted-description"}}]});
    let (frozen, _) = freeze_fixture(input, &store, "fixture", 1).unwrap();
    assert_eq!(frozen["inputs"][0]["data"]["events"][0]["payload"]["kind"], "live_sse");
}

#[test]
fn trace_evidence_must_come_from_inventory_not_inline_claims() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::new(dir.path());
    let input = json!({"slots":[{"input":"projection","kind":"trace_v5","source":"sha256:abc","snapshot":{"invented":true}}]});
    assert!(freeze_fixture(input, &store, "fixture", 1).is_err());
    let request = ("sha256:abc".into(), "rollout-inspector".into());
    let evidence = SealEvidence { visual_id: "fixture", revision: 1, content: &store,
        traces: BTreeMap::from([(request, ResolvedTraceEvidence { payload: json!({"schema_version":"synth.trace-projection.rollout-inspector.v1","rollouts":[]}),
            trace_digest:"sha256:abc".into(), projection_schema:"trace_v5".into(), payload_digest:"abc".into() })]) };
    let input = json!({"slots":[{"input":"projection","kind":"trace_v5","source":"sha256:abc"}]});
    let (frozen, views) = freeze_bindings(input, &evidence).unwrap();
    assert!(views.is_empty());
    assert_eq!(frozen["inputs"][0]["evidence"]["origin"], "trace_inventory");
    assert_eq!(locate_sealed_projections(&frozen)[0]["ref"], "/bindings/inputs/0/data");
}

#[tokio::test]
async fn registry_seal_freezes_real_host_observation_and_reopens_offline() {
    let dir = tempfile::tempdir().unwrap();
    let storage = crate::storage::Storage::open(dir.path()).unwrap();
    let registry = crate::visuals::VisualRegistry::new(storage.database().clone(),
        crate::storage::EventJournal::new(storage.database().clone()), ContentStore::new(storage.content_root()));
    let request = serde_json::from_value(json!({"templateId":"live.eval_stream.v1", "title":"Offline live evidence",
        "bindings":binding(), "metadata":{"qualityGate":{"ready":true,"revision":1}}})).unwrap();
    let (visual, _) = registry.create(request).await.unwrap();
    let identity = registry.certification_identity(visual.id.clone()).await.unwrap();
    let target = visual.id.clone();
    registry.db.run(move |conn| {
        conn.execute("UPDATE visuals SET metadata_json=json_set(metadata_json,'$.qualityGate.certificationIdentity',json(?1)) WHERE id=?2",
            rusqlite::params![identity.to_string(),target])?;
        Ok(())
    }).await.unwrap();
    let declared = crate::visuals::stream_receipt::declared_streams(&visual.bindings);
    crate::visuals::stream_receipt::record_poll_page(&visual.id, 1, &declared, "http://127.0.0.1/events", &json!(events()));
    let (seal, _) = registry.seal(visual.id.clone(), 1).await.unwrap();
    let bundle = registry.get_seal(seal.receipt_digest.clone()).await.unwrap();
    assert_eq!(bundle.data["projection"]["views"][0]["data"]["reward"], 0.75);
    assert_eq!(bundle.data["bindings"]["inputs"][0]["evidence"]["origin"], "host_observation");
    assert!(!bundle.index_html.contains("127.0.0.1"));
    assert!(bundle.index_html.contains("projected envelopes"));
    let reopened = crate::visuals::VisualRegistry::new(storage.database().clone(),
        crate::storage::EventJournal::new(storage.database().clone()), ContentStore::new(storage.content_root()));
    assert_eq!(reopened.get_seal(seal.receipt_digest).await.unwrap().data, bundle.data);
    if let Some(path) = std::env::var_os("WORKSHOP_TEST_SEAL_EXPORT") {
        std::fs::write(path, bundle.index_html).unwrap();
    }
}
