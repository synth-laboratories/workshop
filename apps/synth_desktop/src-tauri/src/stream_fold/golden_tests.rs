use super::*;
use serde_json::json;

// JSON has one numeric type. The JS-generated reference serializes 6.0 as 6,
// whereas serde retains the Rust f64 representation. Compare numbers by value,
// recursively, while keeping every key, array position and null exact.
fn assert_json_value(actual: &Value, expected: &Value, label: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64(), "{label}"),
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(
                a.keys().collect::<Vec<_>>(),
                b.keys().collect::<Vec<_>>(),
                "{label}"
            );
            for (key, value) in a {
                assert_json_value(value, &b[key], &format!("{label}.{key}"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{label}");
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                assert_json_value(a, b, &format!("{label}[{index}]"));
            }
        }
        _ => assert_eq!(actual, expected, "{label}"),
    }
}

#[test]
fn checked_in_live_fixtures_match_shared_golden_in_batch_and_pages() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let golden: Value = serde_json::from_slice(
        &std::fs::read(root.join("visuals/fixtures/live_fold_golden.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(golden["schema"], "synth.live-fold-golden.v1");
    for expected in golden["cases"].as_array().unwrap() {
        let document: Value = if let Some(file) = expected["source"]["file"].as_str() {
            serde_json::from_slice(&std::fs::read(root.join(file)).unwrap()).unwrap()
        } else {
            expected["source"]["inline"].clone()
        };
        let events = document
            .as_array()
            .or_else(|| document["events"].as_array())
            .unwrap();
        for paged in [false, true] {
            let mut fold = LiveFold::new(FoldLimits::retaining());
            let mut accepted = Vec::new();
            for batch in events.chunks(if paged { 1 } else { events.len().max(1) }) {
                for step in fold.accept_batch(batch).steps {
                    if step.verdict.accepted() {
                        accepted.push(json!({"identity":step.identity,"scope":step.scope,"control":step.control}));
                    }
                }
            }
            let p = project_live_eval(fold.events(), None).unwrap();
            let mut gaps = fold.gaps().to_vec();
            gaps.sort_by(|a, b| a.scope.cmp(&b.scope).then(a.after.cmp(&b.after)));
            let actual = json!({"accepted":accepted,"acceptedCount":fold.distinct(),"deliveredCount":fold.delivered(),
                "evidenceCount":fold.evidence_count(),"ready":fold.ready(),"gaps":gaps,
                "conflicts":fold.conflicts().iter().map(|c|c.message.clone()).collect::<Vec<_>>(),
                "lastSequenceByScope":fold.last_sequence_by_scope,
                "projection":{"kinds":p.kinds,"hasLiveFrames":p.has_live_frames,"hasRewardTxt":p.has_reward_txt,
                    "reward":p.reward,"usage":p.usage,"eventCount":p.events.len()}});
            for (key, value) in actual.as_object().unwrap() {
                if key == "projection" {
                    for (field, projected) in value.as_object().unwrap() {
                        assert_json_value(
                            projected,
                            &expected[key][field],
                            &format!("{} paged={paged} projection.{field}", expected["name"]),
                        );
                    }
                    continue;
                }
                assert_eq!(
                    value, &expected[key],
                    "{} paged={paged} field={key}",
                    expected["name"]
                );
            }
        }
    }
}
