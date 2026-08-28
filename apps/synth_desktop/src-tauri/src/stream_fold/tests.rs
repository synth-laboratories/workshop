//! Tests for the one fold.
//!
//! Two layers. The unit tests below pin each rule in the module header against
//! a hand-written case, so a reader can see what the rule means. The golden
//! test at the bottom pins the *whole* fold against every checked-in fixture,
//! captured from the TypeScript mirror and asserted here — which is what stops
//! the two implementations drifting the way the spool and the ingest already
//! had.

use super::*;
use serde_json::json;

fn sequences(values: &[i64]) -> BTreeSet<i64> {
    values.iter().copied().collect()
}

fn gap(scope: &str, after: i64, before: i64) -> SequenceGap {
    SequenceGap {
        scope: scope.to_string(),
        after,
        before,
    }
}

fn fold(events: &[Value]) -> (LiveFold, FoldBatch) {
    let mut state = LiveFold::retaining();
    let batch = state.accept_batch(events.iter());
    (state, batch)
}

// ------------------------------------------------------------ cursor journals

#[test]
fn a_cursor_sees_four_kinds_of_record() {
    assert_eq!(sequence_step(3, 3), SequenceStep::Duplicate);
    assert_eq!(sequence_step(3, 2), SequenceStep::Replay);
    assert_eq!(sequence_step(3, 4), SequenceStep::Next);
    assert_eq!(sequence_step(3, 6), SequenceStep::Gap { expected: 4 });
}

#[test]
fn a_fresh_cursor_expects_sequence_one() {
    assert_eq!(sequence_step(0, 1), SequenceStep::Next);
    assert_eq!(sequence_step(0, 0), SequenceStep::Duplicate);
    assert_eq!(sequence_step(0, 5), SequenceStep::Gap { expected: 1 });
    assert!(is_next_sequence(0, 1));
    assert!(!is_next_sequence(0, 2));
}

#[test]
fn a_monotonic_cursor_advances_only_forward() {
    let mut cursor = 4;
    assert!(!accept_if_ahead(&mut cursor, 4));
    assert!(!accept_if_ahead(&mut cursor, 1));
    assert_eq!(cursor, 4);
    assert!(accept_if_ahead(&mut cursor, 9));
    assert_eq!(cursor, 9, "a hole is the producer's business, not loss");
}

#[test]
fn a_page_cursor_must_match_what_was_committed() {
    assert!(cursor_reconciles(7, 7));
    assert!(!cursor_reconciles(7, 8));
}

// ------------------------------------------------------------------- identity

#[test]
fn a_stream_and_a_sequence_name_an_envelope_outright() {
    let event = json!({"stream_id": "s1", "sequence": 4, "event_id": "e9"});
    assert_eq!(
        envelope_identity(&event, &envelope_scope(&event), 1),
        "s1:4"
    );
}

#[test]
fn identity_keeps_the_producer_lane_so_a_multiplexed_run_does_not_collapse() {
    let one = json!({"event_id": "1", "sequence": 1, "rollout_id": "seed-0"});
    let two = json!({"event_id": "1", "sequence": 1, "rollout_id": "seed-1"});
    assert_ne!(
        envelope_identity(&one, &envelope_scope(&one), 1),
        envelope_identity(&two, &envelope_scope(&two), 2),
        "a bare event_id is rollout-local, never globally unique"
    );
}

#[test]
fn identity_falls_from_event_id_to_sequence_to_kind_and_stamp() {
    assert_eq!(
        envelope_identity(&json!({"event_id": "e1", "sequence": 3}), "roll_a", 1),
        "roll_a:e1"
    );
    assert_eq!(
        envelope_identity(&json!({"sequence": 3}), "roll_a", 1),
        "roll_a:3"
    );
    assert_eq!(
        envelope_identity(&json!({"kind": "tick", "occurred_at": "T0"}), "roll_a", 4),
        "roll_a:tick:T0"
    );
    assert_eq!(
        envelope_identity(&json!({"type": "tick", "ts": "T1"}), "roll_a", 4),
        "roll_a:tick:T1"
    );
    assert_eq!(
        envelope_identity(&json!({"kind": "tick"}), "roll_a", 4),
        "roll_a:tick:4"
    );
    assert_eq!(envelope_identity(&json!({}), "run", 9), "run:event:9");
}

#[test]
fn a_numeric_stamp_is_a_stamp() {
    // Resolved rather than pinned: the renderer reaches `occurred_at ?? ts`
    // through nullish coalescing, which keeps a number, so a producer that
    // stamps with epoch milliseconds got one identity in TypeScript and the
    // delivered ordinal in Rust. A stamp is a stamp whatever its JSON type.
    assert_eq!(
        envelope_identity(&json!({"kind": "tick", "ts": 1700}), "run", 5),
        "run:tick:1700"
    );
}

#[test]
fn an_empty_sequence_does_not_name_an_envelope() {
    assert_eq!(
        envelope_identity(
            &json!({"stream_id": "s", "sequence": "", "event_id": "e1"}),
            "s",
            1
        ),
        "s:e1"
    );
    assert_eq!(
        envelope_identity(
            &json!({"stream_id": "s", "sequence": "", "kind": "tick"}),
            "s",
            2
        ),
        "s:tick:2"
    );
}

#[test]
fn scope_prefers_the_declared_stream_then_the_rollout_lane() {
    assert_eq!(
        envelope_scope(&json!({"stream_id": "s", "rollout_id": "r", "lane": "l", "run_id": "u"})),
        "s"
    );
    assert_eq!(
        envelope_scope(&json!({"rollout_id": "r", "lane": "l"})),
        "r"
    );
    assert_eq!(envelope_scope(&json!({"lane": "l", "run_id": "u"})), "l");
    assert_eq!(envelope_scope(&json!({"run_id": "u"})), "u");
    assert_eq!(envelope_scope(&json!({})), "run");
}

#[test]
fn scope_promotes_identity_carried_in_the_payload() {
    assert_eq!(
        envelope_scope(&json!({"payload": {"rollout_id": "seed-2001"}})),
        "seed-2001"
    );
    assert_eq!(
        envelope_scope(&json!({"payload": {"stream.id": "s7"}})),
        "s7"
    );
}

#[test]
fn a_stream_is_the_declared_stream_or_the_lane() {
    assert_eq!(
        envelope_stream(&json!({"stream_id": "s", "lane": "l"})),
        "s"
    );
    assert_eq!(envelope_stream(&json!({"lane": "l"})), "l");
}

#[test]
fn normalizing_lifts_payload_identity_onto_the_envelope() {
    let normalized = normalize_identity(&json!({"payload": {"rollout_id": "seed-1"}}));
    assert_eq!(normalized["rollout_id"], json!("seed-1"));
    assert_eq!(
        normalized["lane"],
        json!("seed-1"),
        "a lane defaults to the rollout it belongs to"
    );
    assert_eq!(
        normalize_identity(&json!({"kind": "tick"})),
        json!({"kind": "tick"}),
        "an envelope with no identity to promote is returned unchanged"
    );
}

// -------------------------------------------------------------------- control

#[test]
fn control_is_the_known_kinds_and_the_explicit_flag() {
    for kind in CONTROL_KINDS {
        assert!(is_control(&json!({ "kind": kind })), "{kind} is control");
    }
    assert!(is_control(
        &json!({"kind": "rollout.step", "control": true})
    ));
    assert!(!is_control(&json!({"kind": "rollout.step"})));
    assert!(!is_control(
        &json!({"kind": "rollout.step", "control": false})
    ));
}

// ------------------------------------------------------------------- sequence

#[test]
fn sequence_number_wins_over_sequence_and_null_falls_through() {
    assert_eq!(
        numeric_sequence(&json!({"sequence_number": 1, "sequence": 9})),
        Some(1)
    );
    assert_eq!(
        numeric_sequence(&json!({"sequence_number": null, "sequence": 9})),
        Some(9)
    );
}

#[test]
fn an_absent_sequence_is_absent_and_never_zero() {
    assert_eq!(numeric_sequence(&json!({})), None);
    assert_eq!(numeric_sequence(&json!({"sequence": null})), None);
    assert_eq!(numeric_sequence(&json!({"sequence": ""})), None);
}

#[test]
fn numeric_strings_are_read_and_opaque_ones_are_not() {
    assert_eq!(numeric_sequence(&json!({"sequence": "12"})), Some(12));
    assert_eq!(
        numeric_sequence(&json!({"sequence": "suites/x#s0:uuid:frame:0"})),
        None,
        "the real Craftax fixture sequences with opaque strings"
    );
}

#[test]
fn a_fractional_sequence_carries_no_sequence_space() {
    // Rule 6. 1.5 has no successor, so "the number missing after it" is not a
    // claim anyone can make; the lane is unscannable rather than wrong.
    assert_eq!(numeric_sequence(&json!({"sequence": 1.5})), None);
    assert_eq!(numeric_sequence(&json!({"sequence": 3.0})), Some(3));
    assert_eq!(
        sequence_label(&json!({"sequence": 3.0})).as_deref(),
        Some("3")
    );
}

#[test]
fn a_non_scalar_sequence_is_not_a_sequence() {
    assert_eq!(numeric_sequence(&json!({"sequence": {"n": 1}})), None);
    assert_eq!(sequence_label(&json!({"sequence": [1]})), None);
}

// ----------------------------------------------------------------------- gaps

#[test]
fn an_unscanned_scope_has_no_gaps() {
    assert!(scan_gaps("roll_a", &BTreeSet::new()).is_empty());
}

#[test]
fn a_contiguous_run_has_no_gaps() {
    assert!(scan_gaps("roll_a", &sequences(&[1, 2, 3, 4])).is_empty());
}

#[test]
fn every_hole_is_reported_separately_and_in_order() {
    assert_eq!(
        scan_gaps("roll_a", &sequences(&[1, 4, 5, 9])),
        vec![gap("roll_a", 1, 4), gap("roll_a", 5, 9)]
    );
}

#[test]
fn gap_bounds_saturate_at_the_edges() {
    assert_eq!(
        scan_gaps("roll_a", &sequences(&[i64::MIN, i64::MAX])),
        vec![gap("roll_a", i64::MIN, i64::MAX)]
    );
    assert!(scan_gaps("roll_a", &sequences(&[i64::MAX])).is_empty());
}

// ----------------------------------------------------------------- the fold

#[test]
fn a_sequenced_heartbeat_is_not_evidence_and_not_a_hole() {
    // Rule 2. Skipping a control record before recording its sequence made
    // every sequenced heartbeat a permanent phantom gap.
    let (state, batch) = fold(&[
        json!({"kind": "rollout.step", "stream_id": "s", "sequence": 1}),
        json!({"kind": "heartbeat", "stream_id": "s", "sequence": 2}),
        json!({"kind": "rollout.step", "stream_id": "s", "sequence": 3}),
    ]);
    assert!(batch.new_gaps.is_empty(), "{:?}", batch.new_gaps);
    assert!(state.gaps().is_empty());
    assert_eq!(state.delivered(), 3);
    assert_eq!(state.delivered_non_control(), 2);
    assert_eq!(state.evidence_count(), 2);
    assert_eq!(state.events().len(), 2);
}

#[test]
fn the_evidence_high_water_mark_ignores_control_records() {
    // Rule 4, and the one place the receipt and the TypeScript ingest actually
    // disagreed. A stream whose last three records are heartbeats has not
    // advanced its evidence, and a gate that reads `last_sequence` must not be
    // told otherwise.
    let (state, _) = fold(&[
        json!({"kind": "rollout.step", "stream_id": "s", "sequence": 1}),
        json!({"kind": "heartbeat", "stream_id": "s", "sequence": 2}),
        json!({"kind": "heartbeat", "stream_id": "s", "sequence": 3}),
    ]);
    assert_eq!(state.last_sequence("s"), Some(1));
    assert!(
        state.gaps().is_empty(),
        "the heartbeats still fill the space"
    );
}

#[test]
fn control_true_is_honoured_alongside_the_control_kinds() {
    let (state, _) = fold(&[
        json!({"kind": "rollout.step", "control": true, "stream_id": "s", "sequence": 1}),
        json!({"kind": "ping", "stream_id": "s", "sequence": 2}),
        json!({"kind": "stream.subscribed", "stream_id": "s", "sequence": 3}),
    ]);
    assert_eq!(state.delivered(), 3);
    assert_eq!(state.evidence_count(), 0);
    assert_eq!(state.delivered_non_control(), 0);
    assert!(
        state.ready(),
        "a subscription notice is what makes it ready"
    );
}

#[test]
fn one_identity_with_two_bodies_is_a_conflict_and_an_exact_repeat_is_not() {
    let (state, batch) = fold(&[
        json!({"kind": "observation", "stream_id": "s", "sequence": 1, "payload": {"step": 0}}),
        json!({"kind": "observation", "stream_id": "s", "sequence": 1, "payload": {"step": 0}}),
        json!({"kind": "observation", "stream_id": "s", "sequence": 1, "payload": {"step": 7}}),
    ]);
    assert_eq!(state.evidence_count(), 1);
    assert_eq!(state.conflicts().len(), 1);
    assert_eq!(batch.new_conflicts.len(), 1);
    assert_eq!(state.conflicts()[0].identity, "s:1");
    assert_eq!(state.conflicts()[0].scope, "s");
    let verdicts: Vec<FoldVerdict> = batch.steps.iter().map(|step| step.verdict).collect();
    assert_eq!(
        verdicts,
        vec![
            FoldVerdict::Evidence,
            FoldVerdict::Duplicate,
            FoldVerdict::Conflict
        ]
    );
}

#[test]
fn a_producer_declared_digest_decides_equality() {
    let (state, _) = fold(&[
        json!({"kind": "observation", "stream_id": "s", "sequence": 1, "digest": "d", "payload": {"step": 0}}),
        json!({"kind": "observation", "stream_id": "s", "sequence": 1, "digest": "d", "payload": {"step": 7}}),
    ]);
    assert!(
        state.conflicts().is_empty(),
        "the producer says these are the same record"
    );
}

#[test]
fn a_late_envelope_heals_the_gap_it_fills() {
    let mut state = LiveFold::retaining();
    let first = state.accept_batch(
        [
            json!({"kind": "step", "stream_id": "s", "sequence": 1}),
            json!({"kind": "step", "stream_id": "s", "sequence": 3}),
        ]
        .iter(),
    );
    assert_eq!(first.new_gaps, vec![gap("s", 1, 3)]);
    let second =
        state.accept_batch([json!({"kind": "step", "stream_id": "s", "sequence": 2})].iter());
    assert!(second.new_gaps.is_empty());
    assert!(
        state.gaps().is_empty(),
        "the hole was filled, not remembered"
    );
}

#[test]
fn a_multiplexed_run_scans_each_lane_separately() {
    let (state, _) = fold(&[
        json!({"kind": "step", "rollout_id": "seed-0", "sequence": 1}),
        json!({"kind": "step", "rollout_id": "seed-1", "sequence": 1}),
        json!({"kind": "step", "rollout_id": "seed-0", "sequence": 3}),
    ]);
    assert_eq!(state.gaps(), &[gap("seed-0", 1, 3)]);
    assert_eq!(state.evidence_count(), 3);
}

#[test]
fn tracking_stops_at_its_bound_and_the_fold_says_so() {
    let mut state = LiveFold::new(FoldLimits {
        max_identities: 2,
        max_sequences_per_scope: 2,
        max_defects: 1,
        retain_events: false,
    });
    state.accept_batch(
        (1..=5)
            .map(|n| json!({"kind": "step", "stream_id": "s", "sequence": n}))
            .collect::<Vec<_>>()
            .iter(),
    );
    assert!(state.truncated());
    assert_eq!(
        state.delivered(),
        5,
        "a bound truncates the bookkeeping, never the delivered count"
    );
}

// --------------------------------------------------------------- the cutoff

#[test]
fn a_cutoff_is_a_prefix_length_per_stream() {
    let events: Vec<Value> = vec![
        json!({"kind": "step", "stream_id": "a", "sequence": "x0"}),
        json!({"kind": "step", "stream_id": "b", "sequence": "y0"}),
        json!({"kind": "step", "stream_id": "a", "sequence": "x1"}),
        json!({"kind": "step", "stream_id": "b", "sequence": "y1"}),
    ];
    let cutoff = CursorVector::new([("a".into(), 1), ("b".into(), 2)]);
    let projection = project_live_eval(&events, Some(&cutoff)).expect("project");
    let streams: Vec<String> = projection.events.iter().map(envelope_stream).collect();
    assert_eq!(streams, vec!["a", "b", "b"]);
    assert_eq!(cutoff.total(), 3);
}

#[test]
fn an_unnamed_stream_is_excluded_rather_than_whole() {
    let events: Vec<Value> = vec![
        json!({"kind": "step", "stream_id": "a"}),
        json!({"kind": "step", "stream_id": "b"}),
    ];
    let cutoff = CursorVector::new([("a".into(), 1)]);
    let projection = project_live_eval(&events, Some(&cutoff)).expect("project");
    assert_eq!(projection.events.len(), 1);
}

#[test]
fn the_folds_own_cursor_addresses_everything_it_folded() {
    let (state, _) = fold(&[
        json!({"kind": "step", "stream_id": "a", "sequence": 1}),
        json!({"kind": "heartbeat", "stream_id": "a", "sequence": 2}),
        json!({"kind": "step", "stream_id": "b", "sequence": 1}),
    ]);
    assert_eq!(
        state.cursor(),
        CursorVector::new([("a".into(), 1), ("b".into(), 1)]),
        "control envelopes are not addressable evidence"
    );
}

#[test]
fn the_whole_prefix_reproduces_the_uncut_projection() {
    let events: Vec<Value> = vec![
        json!({"kind": "frame", "stream_id": "a", "sequence": 1}),
        json!({"kind": "verifier", "stream_id": "a", "sequence": 2, "payload": {"reward.txt": 0.5}}),
    ];
    let mut state = LiveFold::retaining();
    state.accept_batch(events.iter());
    let whole = project_live_eval(state.events(), None).expect("project");
    let at_cursor = project_live_eval(state.events(), Some(&state.cursor())).expect("project");
    assert_eq!(whole.kinds, at_cursor.kinds);
    assert_eq!(whole.reward, at_cursor.reward);
}

// ----------------------------------------------------------- the projection

#[test]
fn the_projection_reads_reward_from_the_last_verifier_then_the_terminal() {
    let events: Vec<Value> = vec![
        json!({"kind": "verifier", "payload": {"reward.txt": 0.25}}),
        json!({"kind": "verifier", "payload": {"reward.txt": 0.75}}),
    ];
    assert_eq!(
        project_live_eval(&events, None).expect("project").reward,
        Some(0.75)
    );
    let terminal: Vec<Value> = vec![json!({"kind": "eval.run.terminal", "payload": {"value": 1}})];
    assert_eq!(
        project_live_eval(&terminal, None).expect("project").reward,
        Some(1.0)
    );
    assert_eq!(
        project_live_eval(&[json!({"kind": "frame"})], None)
            .expect("project")
            .reward,
        None,
        "a missing reward stays missing rather than becoming zero"
    );
}

#[test]
fn the_projection_finds_reward_txt_at_any_depth() {
    let nested: Vec<Value> =
        vec![json!({"kind": "verifier", "payload": {"files": [{"reward.txt": 1}]}})];
    assert!(
        project_live_eval(&nested, None)
            .expect("project")
            .has_reward_txt
    );
    assert!(
        !project_live_eval(&[json!({"kind": "frame"})], None)
            .expect("project")
            .has_reward_txt
    );
}

#[test]
fn the_projection_takes_usage_from_the_last_envelope_carrying_any() {
    let events: Vec<Value> = vec![
        json!({"kind": "step", "payload": {"usage": {"total_tokens": 1}}}),
        json!({"kind": "step", "payload": {"usage": {"prompt_tokens": 9, "cost_usd": 0.5}}}),
        json!({"kind": "step", "payload": {}}),
    ];
    let usage = project_live_eval(&events, None)
        .expect("project")
        .usage
        .expect("usage");
    assert_eq!(usage.prompt_tokens, Some(9.0));
    assert_eq!(usage.cost_usd, Some(0.5));
    assert_eq!(
        usage.total_tokens, None,
        "a field the producer omitted stays absent"
    );
}

#[test]
fn the_projection_drops_control_envelopes_it_was_handed() {
    let projection = project_live_eval(
        &[
            json!({"kind": "heartbeat"}),
            json!({"kind": "frame"}),
            json!({"kind": "step", "control": true}),
        ],
        None,
    )
    .expect("project");
    assert_eq!(projection.kinds, vec!["frame"]);
    assert!(projection.has_live_frames);
}

#[test]
fn the_projection_refuses_to_carry_a_capability_blob() {
    let leaked: Vec<Value> = vec![json!({"kind": "step", "payload": {"capability_blob": "…"}})];
    assert!(project_live_eval(&leaked, None).is_err());
}

// ------------------------------------------------------------------- golden

/// The golden capture, and the fixtures it was taken over.
///
/// Regenerate with `node visuals/tests/live_fold_golden_gen.mjs`. A diff here
/// is either a deliberate change to the fold — in which case regenerate and
/// review the diff, which is the point — or the two implementations drifting
/// apart, which is the thing this exists to catch.
#[test]
fn golden_fixtures_fold_the_same_way_in_both_implementations() {
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let golden_path = repo.join("visuals/fixtures/live_fold_golden.json");
    let golden: Value = serde_json::from_slice(
        &std::fs::read(&golden_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", golden_path.display())),
    )
    .expect("parse the golden");
    assert_eq!(
        golden["schema"], "synth.live-fold-golden.v1",
        "a reader that does not know the schema must refuse it rather than guess"
    );
    let cases = golden["cases"].as_array().expect("golden cases");
    assert!(cases.len() >= 8, "the golden lost its fixtures");

    for case in cases {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let events: Vec<Value> = match case["source"].get("file").and_then(Value::as_str) {
            Some(file) => {
                let path = repo.join(file);
                let parsed: Value = serde_json::from_slice(
                    &std::fs::read(&path)
                        .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
                )
                .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
                envelopes_from_fixture(&parsed)
            }
            None => case["source"]["inline"]
                .as_array()
                .unwrap_or_else(|| panic!("{name}: neither a file nor inline events"))
                .clone(),
        };

        let mut state = LiveFold::retaining();
        let batch = state.accept_batch(events.iter());

        assert_eq!(
            state.delivered(),
            case["deliveredCount"].as_u64().unwrap(),
            "{name}: delivered"
        );
        assert_eq!(
            state.distinct(),
            case["acceptedCount"].as_u64().unwrap(),
            "{name}: accepted"
        );
        assert_eq!(
            state.evidence_count(),
            case["evidenceCount"].as_u64().unwrap(),
            "{name}: evidence"
        );
        assert_eq!(
            state.ready(),
            case["ready"].as_bool().unwrap(),
            "{name}: ready"
        );

        let accepted: Vec<(String, String, bool)> = batch
            .steps
            .iter()
            .filter(|step| step.verdict.accepted())
            .map(|step| (step.identity.clone(), step.scope.clone(), step.control))
            .collect();
        let expected: Vec<(String, String, bool)> = case["accepted"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["identity"].as_str().unwrap().to_string(),
                    row["scope"].as_str().unwrap().to_string(),
                    row["control"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(accepted, expected, "{name}: identity, scope and control");

        let mut gaps: Vec<(&str, i64, i64)> = state
            .gaps()
            .iter()
            .map(|gap| (gap.scope.as_str(), gap.after, gap.before))
            .collect();
        gaps.sort();
        let expected_gaps: Vec<(&str, i64, i64)> = case["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["scope"].as_str().unwrap(),
                    row["after"].as_i64().unwrap(),
                    row["before"].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(gaps, expected_gaps, "{name}: gaps");

        let mut conflicts: Vec<&str> = state
            .conflicts()
            .iter()
            .map(|conflict| conflict.message.as_str())
            .collect();
        conflicts.sort_unstable();
        let expected_conflicts: Vec<&str> = case["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap())
            .collect();
        assert_eq!(conflicts, expected_conflicts, "{name}: conflicts");

        let expected_last: BTreeMap<String, i64> = case["lastSequenceByScope"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(scope, value)| (scope.clone(), value.as_i64().unwrap()))
            .collect();
        assert_eq!(
            state.last_sequence_by_scope(),
            &expected_last,
            "{name}: evidence high-water marks"
        );

        let projection = project_live_eval(state.events(), None)
            .unwrap_or_else(|error| panic!("{name}: project: {error}"));
        let expected_projection = &case["projection"];
        assert_eq!(
            projection.events.len() as u64,
            expected_projection["eventCount"].as_u64().unwrap(),
            "{name}: projected rows"
        );
        let expected_kinds: Vec<&str> = expected_projection["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kind| kind.as_str().unwrap())
            .collect();
        assert_eq!(projection.kinds, expected_kinds, "{name}: kinds");
        assert_eq!(
            projection.has_live_frames,
            expected_projection["hasLiveFrames"].as_bool().unwrap(),
            "{name}: has_live_frames"
        );
        assert_eq!(
            projection.has_reward_txt,
            expected_projection["hasRewardTxt"].as_bool().unwrap(),
            "{name}: has_reward_txt"
        );
        assert_eq!(
            projection.reward,
            expected_projection["reward"].as_f64(),
            "{name}: reward"
        );
        match expected_projection["usage"].as_object() {
            None => assert!(projection.usage.is_none(), "{name}: usage"),
            Some(expected_usage) => {
                let field = |key: &str| expected_usage.get(key).and_then(Value::as_f64);
                let usage = projection.usage.unwrap_or_else(|| panic!("{name}: usage"));
                assert_eq!(usage.prompt_tokens, field("prompt_tokens"), "{name}: usage");
                assert_eq!(
                    usage.completion_tokens,
                    field("completion_tokens"),
                    "{name}: usage"
                );
                assert_eq!(usage.total_tokens, field("total_tokens"), "{name}: usage");
                assert_eq!(usage.cost_usd, field("cost_usd"), "{name}: usage");
            }
        }
    }
}

/// The three shapes a fixture file uses for its envelope array, read the way
/// the generator reads them.
fn envelopes_from_fixture(parsed: &Value) -> Vec<Value> {
    if let Some(rows) = parsed.as_array() {
        return rows.clone();
    }
    parsed
        .get("events")
        .or_else(|| parsed.pointer("/page/events"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}
