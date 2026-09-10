use super::*;
use serde_json::json;

fn declared() -> DeclaredStreams {
    DeclaredStreams { streams: vec![DeclaredStream { stream_id: "fixture".into(), poll_url: "http://127.0.0.1/events".into(), sse_url: None }], missing_transport: vec![] }
}
fn id() -> String { uuid::Uuid::new_v4().to_string() }
fn poll(id: &str, revision: i64, events: Value) -> StreamReceipt {
    let streams = declared();
    record_poll_attempt(id, revision, &streams, &streams.streams[0].poll_url);
    record_poll_page(id, revision, &streams, &streams.streams[0].poll_url, &events);
    receipt(id, revision, &streams)
}

#[test]
fn concurrent_poll_snapshots_pair_receipt_with_the_exact_retained_prefix() {
    let id = id();
    let writer_id = id.clone();
    let writer = std::thread::spawn(move || {
        for sequence in 1..=100 {
            poll(&writer_id, 1, json!([{"sequence":sequence,"kind":"frame"}]));
            std::thread::yield_now();
        }
    });
    for _ in 0..100 {
        let (receipt, events, truncated) = evidence_snapshot(&id, 1, &declared());
        assert!(!truncated);
        assert_eq!(receipt.recovered, events.len() as u64);
        assert_eq!(receipt.streams[0].poll_responses, events.len() as u64);
        std::thread::yield_now();
    }
    writer.join().unwrap();
    let (receipt, events, truncated) = evidence_snapshot(&id, 1, &declared());
    assert!(!truncated);
    assert_eq!(receipt.recovered, 100);
    assert_eq!(events.len(), 100);
    assert_eq!(events.last().unwrap()["sequence"], 100);
}

#[test]
fn folded_lanes_control_gaps_and_conflicts_remain_distinct() {
    let mut fold = LiveFold::default();
    let events = vec![json!({"rollout_id":"a","sequence":1,"kind":"frame"}),
        json!({"rollout_id":"b","sequence":1,"kind":"frame"}),
        json!({"rollout_id":"a","sequence":2,"kind":"heartbeat"}),
        json!({"rollout_id":"a","sequence":3,"kind":"frame"})];
    fold.accept_batch(&events);
    fold.accept_batch(&events);
    assert_eq!(fold.evidence_count(), 3);
    assert_eq!(fold.delivered(), 8);
    assert!(fold.gaps().is_empty());
    fold.accept(&json!({"rollout_id":"a","sequence":5,"kind":"heartbeat"}));
    assert_eq!(fold.last_sequence("a"), Some(3));
    assert_eq!(fold.gaps().len(), 1);
    fold.accept(&json!({"rollout_id":"a","sequence":4,"kind":"frame"}));
    assert!(fold.gaps().is_empty());
    fold.accept(&json!({"rollout_id":"a","sequence":3,"kind":"frame","payload":{"changed":true}}));
    assert_eq!(fold.conflicts().len(), 1);
}

#[test]
fn accounting_limit_does_not_invent_distinct_evidence() {
    let mut fold = LiveFold::new(FoldLimits { max_identities: 1, ..FoldLimits::default() });
    fold.accept(&json!({"sequence":1,"kind":"frame"}));
    for _ in 0..3 { fold.accept(&json!({"sequence":2,"kind":"frame"})); }
    assert_eq!(fold.evidence_count(), 1);
    assert_eq!(fold.distinct(), 1);
    assert_eq!(fold.delivered(), 4);
    assert!(fold.truncated());
}

#[test]
fn certification_requires_observed_distinct_evidence_from_every_stream() {
    let id = id();
    assert!(certification(&receipt(&id, 1, &declared()), 1).is_err());
    let controls = poll(&id, 1, json!([{"sequence":1,"kind":"heartbeat"}]));
    assert!(certification(&controls, 1).is_err());
    assert!(crate::visuals::live_eval::observed_projection(&id, 1, None).is_none());
    let valid = poll(&id, 1, json!([{"sequence":2,"kind":"frame"}]));
    assert!(certification(&valid, 1).is_ok());
    assert!(certification(&valid, 2).is_err());
    let replay = poll(&id, 1, json!([{"sequence":2,"kind":"frame"}]));
    assert!(certification(&replay, 2).is_err());
    let conflict = poll(&id, 1, json!([{"sequence":2,"kind":"frame","payload":1}]));
    assert!(certification(&conflict, 1).is_err());
    let mut missing = valid.clone();
    missing.declared_stream_count += 1;
    assert!(certification(&missing, 1).is_err());
    missing = valid.clone();
    missing.streams_missing_transport.push("missing".into());
    assert!(certification(&missing, 1).is_err());
    missing = valid;
    missing.tracking_truncated = true;
    assert!(certification(&missing, 1).is_err());
}

#[test]
fn old_revision_reads_and_late_pages_do_not_erase_newer_evidence() {
    let id = id();
    let page = json!([{"sequence":1,"kind":"frame","payload":{"rollout_id":"lane-a"}}]);
    poll(&id, 2, page.clone());
    assert!(!receipt(&id, 1, &declared()).observed);
    poll(&id, 1, page);
    let evidence = observed_evidence(&id, 2, "fixture").unwrap().0;
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["rollout_id"], "lane-a");
    assert_eq!(receipt(&id, 2, &declared()).recovered, 1);
    let mut rebound = declared();
    rebound.streams[0].poll_url = "http://127.0.0.1/replacement".into();
    assert!(!receipt(&id, 2, &rebound).observed);
    assert!(observed_evidence(&id, 2, "fixture").is_none());
}

#[test]
fn retained_evidence_stays_a_prefix_after_byte_limit() {
    let mut state = VisualState::new(1, &declared());
    assert!(state.retain_evidence("fixture", &json!({"kind":"frame"})));
    assert!(!state.retain_evidence("fixture", &json!({"payload":"x".repeat(MAX_RETAINED_BYTES)})));
    assert!(!state.retain_evidence("fixture", &json!({"kind":"verifier"})));
    assert_eq!(state.evidence.len(), 1);
    assert!(state.evidence_books["fixture"].truncated);
}

#[test]
fn projection_truncation_survives_a_quiet_stream_response() {
    let id = id();
    poll(&id, 1, json!([{"sequence":1,"kind":"frame"}]));
    {
        let mut map = store();
        let state = entry(&mut map, &id, 1, &declared());
        state.evidence_books.entry("another-stream".into()).or_default().truncated = true;
    }
    let streams = declared();
    let outcome = record_poll_page(&id, 1, &streams, &streams.streams[0].poll_url, &json!([]));
    assert!(outcome.evidence_truncated);
}

#[test]
fn projection_preserves_missing_usage_and_opaque_cutoff() {
    let mut fold = LiveFold::retaining();
    fold.accept(&json!({"stream_id":"a","sequence":"opaque-a","kind":"frame"}));
    fold.accept(&json!({"stream_id":"a","sequence":"opaque-b","kind":"verifier","payload":{"reward.txt":0.5}}));
    let cutoff = stream_fold::CursorVector::new([("a".into(), 1)]);
    let prefix = stream_fold::project_live_eval(fold.events(), Some(&cutoff)).unwrap();
    assert!(prefix.has_live_frames);
    assert_eq!(prefix.reward, None);
    assert_eq!(prefix.usage, None);
    assert_eq!(stream_fold::project_live_eval(fold.events(), None).unwrap().reward, Some(0.5));
    assert!(stream_fold::project_live_eval(&[json!({"payload":{"capability_blob":"private"}})], None).is_err());
}
