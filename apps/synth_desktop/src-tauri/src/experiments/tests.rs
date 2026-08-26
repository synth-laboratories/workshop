use super::*;
use rusqlite::Connection;

use crate::contract::specta::OpaqueJson;

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::storage::migrations::apply_migrations(&conn).unwrap();
    conn
}

#[test]
fn five_concurrent_workflow_identities_stay_isolated() {
    let conn = database();
    for index in 1..=5 {
        let session = format!("session_{index}");
        let campaign = format!("camp_{index}");
        let optimizer = format!("opt_{index}");
        attach(
            &conn,
            &session,
            MEMBER_CAMPAIGN,
            &campaign,
            "2026-08-17T00:00:00Z",
            &format!("Eval {index}"),
        )
        .unwrap();
        attach(
            &conn,
            &session,
            MEMBER_OPTIMIZER,
            &optimizer,
            "2026-08-17T00:00:01Z",
            &format!("GEPA {index}"),
        )
        .unwrap();
    }
    for index in 1..=5 {
        let group = load_for_session(&conn, &format!("session_{index}"))
            .unwrap()
            .expect("each task owns an experiment group");
        assert_eq!(group.session_id, format!("session_{index}"));
        assert_eq!(group.members.len(), 2);
        assert_eq!(group.nodes.len(), 2);
        assert!(group
            .members
            .iter()
            .all(|member| member.member_id.ends_with(&index.to_string())));
        assert!(!group.members.iter().any(|member| member
            .member_id
            .contains(&(index % 5 + 1).to_string())
            && member.member_id != format!("camp_{index}")
            && member.member_id != format!("opt_{index}")));
    }
    let session_1 = load_for_session(&conn, "session_1").unwrap().unwrap();
    assert!(!session_1
        .members
        .iter()
        .any(|member| member.member_id == "camp_2" || member.member_id == "opt_2"));
}

#[test]
fn attaching_optimizer_and_eval_writes_member_nodes_and_evaluated_edge() {
    let conn = database();
    attach(
        &conn,
        "session_1",
        MEMBER_OPTIMIZER,
        "opt_1",
        "2026-08-17T00:00:00Z",
        "GEPA",
    )
    .unwrap();
    let group = attach(
        &conn,
        "session_1",
        MEMBER_CAMPAIGN,
        "camp_1",
        "2026-08-17T00:00:01Z",
        "Eval",
    )
    .unwrap();
    assert_eq!(group.members.len(), 2);
    assert_eq!(group.nodes.len(), 2);
    assert!(group.nodes.iter().any(|node| node.kind == MEMBER_OPTIMIZER));
    assert!(group.nodes.iter().any(|node| node.kind == MEMBER_CAMPAIGN));
    assert_eq!(group.edges.len(), 1);
    assert_eq!(group.edges[0].relation, "evaluated");
    let optimizer = group
        .nodes
        .iter()
        .find(|node| node.kind == MEMBER_OPTIMIZER)
        .unwrap();
    let eval = group
        .nodes
        .iter()
        .find(|node| node.kind == MEMBER_CAMPAIGN)
        .unwrap();
    assert_eq!(group.edges[0].source_node_id, optimizer.id);
    assert_eq!(group.edges[0].target_node_id, eval.id);
    let replay = attach(
        &conn,
        "session_1",
        MEMBER_CAMPAIGN,
        "camp_1",
        "2026-08-17T00:00:02Z",
        "Eval",
    )
    .unwrap();
    assert_eq!(replay.nodes.len(), 2);
    assert_eq!(replay.edges.len(), 1);
}

#[test]
fn attaching_the_same_member_twice_is_idempotent() {
    let conn = database();
    attach(
        &conn,
        "session_1",
        MEMBER_CAMPAIGN,
        "camp_1",
        "2026-08-17T00:00:00Z",
        "Eval",
    )
    .unwrap();
    let again = attach(
        &conn,
        "session_1",
        MEMBER_CAMPAIGN,
        "camp_1",
        "2026-08-17T00:00:02Z",
        "Eval",
    )
    .unwrap();
    assert_eq!(again.members.len(), 1);
    assert_eq!(again.nodes.len(), 1);
    assert_eq!(again.nodes[0].kind, MEMBER_CAMPAIGN);
    assert!(again.edges.is_empty());
    assert!(again.nodes.iter().all(|node| node.cost_usd.is_none()));
    assert!(again.nodes.iter().all(|node| node.metrics.is_none()));
}

#[test]
fn settling_a_member_updates_summary_nodes_and_preserves_missing_cost() {
    let conn = database();
    attach(
        &conn,
        "session_1",
        MEMBER_CAMPAIGN,
        "camp_1",
        "2026-08-17T00:00:00Z",
        "Craftax compare",
    )
    .unwrap();
    settle_member(
        &conn,
        MEMBER_CAMPAIGN,
        "camp_1",
        "complete",
        "CRAFTAX-EMBER-0824",
        Some("gpt-5.6-luna"),
        &serde_json::json!({"reward":{"mean":null},"sampleSize":2}),
        &["/rollouts/a/trace".into(), "/rollouts/b/trace".into()],
        "2026-08-17T00:01:00Z",
    )
    .unwrap();
    let group = load_for_session(&conn, "session_1").unwrap().unwrap();
    assert_eq!(group.status, "completed");
    assert_eq!(group.task.as_deref(), Some("CRAFTAX-EMBER-0824"));
    assert_eq!(group.model.as_deref(), Some("gpt-5.6-luna"));
    let result = group
        .nodes
        .iter()
        .find(|node| node.kind == MEMBER_CAMPAIGN)
        .unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(result.trace_refs.len(), 2);
    assert!(result.cost_usd.is_none());
    assert_eq!(
        result.metrics.as_ref().unwrap().0["reward"]["mean"],
        serde_json::Value::Null
    );
}

#[test]
fn search_and_reopen_return_the_same_durable_identity() {
    let conn = database();
    let created = attach(
        &conn,
        "task_craftax",
        MEMBER_CAMPAIGN,
        "run_1",
        "2026-08-24T12:00:00Z",
        "Craftax prompt variant",
    )
    .unwrap();
    let found = list(&conn, Some("craftax")).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, created.id);
    assert_eq!(get(&conn, &created.id).unwrap().unwrap().id, created.id);
}

#[test]
fn evidence_references_are_typed_idempotent_and_durable() {
    let conn = database();
    let experiment = attach(
        &conn,
        "task_1",
        MEMBER_CAMPAIGN,
        "camp_1",
        "2026-08-24T12:00:00Z",
        "Craftax",
    )
    .unwrap();
    let request = ExperimentEvidenceAttachRequest {
        experiment_id: experiment.id.clone(),
        session_id: Some("task_1".into()),
        node_id: None,
        evidence_id: "trace:craftax:rollout_1".into(),
        kind: "trace".into(),
        label: "Seed 0 trace".into(),
        digest: Some("sha256:seal".into()),
        container_id: Some("craftax".into()),
        rollout_id: Some("rollout_1".into()),
        trace_id: None,
        visual_id: None,
        artifact_uri: None,
        metadata: Some(OpaqueJson(serde_json::json!({"eventCount":75}))),
        attached_at: "2026-08-24T12:01:00Z".into(),
    };
    attach_evidence(&conn, request.clone()).unwrap();
    let replay = ExperimentEvidenceAttachRequest {
        attached_at: "2026-08-24T12:02:00Z".into(),
        ..request.clone()
    };
    let group = attach_evidence(&conn, replay).unwrap();
    let result = group
        .nodes
        .iter()
        .find(|node| node.kind == MEMBER_CAMPAIGN)
        .unwrap();
    assert_eq!(result.evidence_refs.len(), 1);
    assert_eq!(
        result.evidence_refs[0].rollout_id.as_deref(),
        Some("rollout_1")
    );
    assert_eq!(result.evidence_refs[0].metadata.0["eventCount"], 75);
    let mut conflict = request;
    conflict.trace_id = Some("different".into());
    assert!(attach_evidence(&conn, conflict)
        .unwrap_err()
        .to_string()
        .contains("different content"));
    let events: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind='experiment.evidence.attached'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 1);
}

#[test]
fn direct_agent_lifecycle_is_indexed_idempotent_and_session_owned() {
    let conn = database();
    let request = ExperimentCreateRequest {
        session_id: "task_direct".into(),
        request_id: "CRAFTAX-EMBER-0824".into(),
        title: "Craftax survival comparison".into(),
        task: Some("CRAFTAX-EMBER-0824".into()),
        model: Some("gpt-5.6-luna".into()),
        created_at: "2026-08-24T12:00:00Z".into(),
    };
    let first = create(&conn, request.clone()).unwrap();
    let replay = create(
        &conn,
        ExperimentCreateRequest {
            created_at: "2026-08-24T12:01:00Z".into(),
            ..request
        },
    )
    .unwrap();
    assert_eq!(first.id, replay.id);
    assert_eq!(replay.nodes.len(), 1);
    assert_eq!(replay.nodes[0].kind, MEMBER_DIRECT);
    let done = finalize(
        &conn,
        ExperimentFinalizeRequest {
            experiment_id: first.id.clone(),
            session_id: "task_direct".into(),
            status: "completed".into(),
            result: OpaqueJson(
                serde_json::json!({"baselineReward":0.5,"variantReward":0.5,"delta":0}),
            ),
            assessment: Some(OpaqueJson(
                serde_json::json!({"verdict":"insufficient evidence"}),
            )),
            finalized_at: "2026-08-24T12:02:00Z".into(),
        },
    )
    .unwrap();
    assert_eq!(done.status, "completed");
    assert_eq!(list(&conn, Some("craftax")).unwrap().len(), 1);
    assert!(finalize(
        &conn,
        ExperimentFinalizeRequest {
            experiment_id: first.id,
            session_id: "another_task".into(),
            status: "failed".into(),
            result: OpaqueJson(serde_json::json!({})),
            assessment: None,
            finalized_at: "2026-08-24T12:03:00Z".into(),
        },
    )
    .is_err());
}

#[test]
fn child_experiment_is_a_new_row_with_follow_up_and_becomes_the_attach_target() {
    let conn = database();
    let parent = create(
        &conn,
        ExperimentCreateRequest {
            session_id: "task_child".into(),
            request_id: "parent-req".into(),
            title: "Parent study".into(),
            task: Some("CRAFTAX-EMBER-0824".into()),
            model: Some("gpt-5.6-luna".into()),
            created_at: "2026-08-26T00:00:00Z".into(),
        },
    )
    .unwrap();
    let child = create_child(
        &conn,
        ExperimentChildCreateRequest {
            parent_experiment_id: parent.id.clone(),
            session_id: Some("task_child".into()),
            request_id: "child-req".into(),
            title: "Follow-up study".into(),
            task: None,
            model: None,
            created_at: "2026-08-26T00:01:00Z".into(),
            relation: None,
        },
    )
    .unwrap();
    assert_ne!(parent.id, child.id);
    assert_eq!(child.session_id, parent.session_id);
    assert_eq!(child.task.as_deref(), Some("CRAFTAX-EMBER-0824"));
    assert_eq!(child.model.as_deref(), Some("gpt-5.6-luna"));
    let parent = get(&conn, &parent.id).unwrap().unwrap();
    assert_eq!(parent.lineage.len(), 1);
    assert_eq!(parent.lineage[0].relation, "follow_up");
    assert_eq!(parent.lineage[0].target_experiment_id, child.id);
    super::registry::insert_lineage(
        &conn,
        &parent.id,
        &child.id,
        "forked_from",
        "2026-08-26T00:01:30Z",
    )
    .unwrap();
    let parent = get(&conn, &parent.id).unwrap().unwrap();
    assert!(parent
        .lineage
        .iter()
        .any(|edge| edge.relation == "forked_from" && edge.target_experiment_id == child.id));
    let replay = create_child(
        &conn,
        ExperimentChildCreateRequest {
            parent_experiment_id: parent.id.clone(),
            session_id: Some("task_child".into()),
            request_id: "child-req".into(),
            title: "Follow-up study".into(),
            task: None,
            model: None,
            created_at: "2026-08-26T00:02:00Z".into(),
            relation: None,
        },
    )
    .unwrap();
    assert_eq!(replay.id, child.id);
    assert_eq!(list(&conn, Some("study")).unwrap().len(), 2);
    attach(
        &conn,
        "task_child",
        MEMBER_OPTIMIZER,
        "opt_child",
        "2026-08-26T00:03:00Z",
        "GEPA",
    )
    .unwrap();
    let active = load_for_session(&conn, "task_child").unwrap().unwrap();
    assert_eq!(active.id, child.id);
    assert!(active
        .members
        .iter()
        .any(|member| member.member_id == "opt_child"));
    let parent = get(&conn, &parent.id).unwrap().unwrap();
    assert!(!parent
        .members
        .iter()
        .any(|member| member.member_id == "opt_child"));
    activate(&conn, "task_child", &parent.id).unwrap();
    let retargeted = load_for_session(&conn, "task_child").unwrap().unwrap();
    assert_eq!(retargeted.id, parent.id);
}

#[test]
fn get_returns_the_named_experiment_not_the_session_primary() {
    let conn = database();
    let parent = create(
        &conn,
        ExperimentCreateRequest {
            session_id: "task_named".into(),
            request_id: "named-parent".into(),
            title: "Named parent".into(),
            task: None,
            model: None,
            created_at: "2026-08-26T00:00:00Z".into(),
        },
    )
    .unwrap();
    let child = create_child(
        &conn,
        ExperimentChildCreateRequest {
            parent_experiment_id: parent.id.clone(),
            session_id: None,
            request_id: "named-child".into(),
            title: "Named child".into(),
            task: None,
            model: None,
            created_at: "2026-08-26T00:01:00Z".into(),
            relation: None,
        },
    )
    .unwrap();
    assert_eq!(get(&conn, &parent.id).unwrap().unwrap().id, parent.id);
    assert_eq!(get(&conn, &child.id).unwrap().unwrap().id, child.id);
    assert_ne!(
        get(&conn, &parent.id).unwrap().unwrap().id,
        get(&conn, &child.id).unwrap().unwrap().id
    );
}

#[test]
fn gepa_candidate_ids_upsert_onto_the_optimizer_run_not_a_member_kind() {
    let conn = database();
    attach(
        &conn,
        "session_gepa",
        MEMBER_OPTIMIZER,
        "opt_gepa",
        "2026-08-26T00:00:00Z",
        "GEPA",
    )
    .unwrap();
    upsert_candidate(
        &conn,
        CandidateUpsert {
            optimizer_run_id: "opt_gepa".into(),
            producer_candidate_id: "gepa_seed".into(),
            kind: Some("prompt_overlay".into()),
            protocol_id: Some("prompt_overlay.v1".into()),
            status: Some("registered".into()),
            parent_ids: vec![],
            metrics: None,
            content_digest: None,
            compared_with: None,
            promoted_to: None,
            at: "2026-08-26T00:01:00Z".into(),
        },
    )
    .unwrap();
    upsert_candidate(
        &conn,
        CandidateUpsert {
            optimizer_run_id: "opt_gepa".into(),
            producer_candidate_id: "gepa_child".into(),
            kind: Some("prompt_overlay".into()),
            protocol_id: Some("prompt_overlay.v1".into()),
            status: Some("accepted".into()),
            parent_ids: vec!["gepa_seed".into()],
            metrics: Some(serde_json::json!({"train_reward": 0.4})),
            content_digest: None,
            compared_with: None,
            promoted_to: None,
            at: "2026-08-26T00:02:00Z".into(),
        },
    )
    .unwrap();
    upsert_candidate(
        &conn,
        CandidateUpsert {
            optimizer_run_id: "opt_gepa".into(),
            producer_candidate_id: "gepa_seed".into(),
            kind: None,
            protocol_id: None,
            status: Some("evaluated".into()),
            parent_ids: vec![],
            metrics: Some(serde_json::json!({"train_reward": 0.7})),
            content_digest: None,
            compared_with: None,
            promoted_to: None,
            at: "2026-08-26T00:03:00Z".into(),
        },
    )
    .unwrap();
    let group = load_for_session(&conn, "session_gepa").unwrap().unwrap();
    assert!(group.nodes.iter().all(|node| node.kind != "candidate"));
    let run = group
        .nodes
        .iter()
        .find(|node| node.kind == MEMBER_OPTIMIZER)
        .unwrap();
    assert_eq!(run.candidates.len(), 2);
    let seed = run
        .candidates
        .iter()
        .find(|candidate| candidate.producer_candidate_id == "gepa_seed")
        .unwrap();
    let child = run
        .candidates
        .iter()
        .find(|candidate| candidate.producer_candidate_id == "gepa_child")
        .unwrap();
    assert_eq!(seed.optimizer_run_id, "opt_gepa");
    assert_eq!(seed.kind.as_deref(), Some("prompt_overlay"));
    assert_eq!(seed.protocol_id.as_deref(), Some("prompt_overlay.v1"));
    assert_eq!(seed.status.as_deref(), Some("evaluated"));
    assert_eq!(seed.metrics.as_ref().unwrap().0["train_reward"], 0.7);
    assert_eq!(child.parent_ids, vec!["gepa_seed".to_string()]);
    let reopened = get(&conn, &group.id).unwrap().unwrap();
    let reopened_run = reopened
        .nodes
        .iter()
        .find(|node| node.kind == MEMBER_OPTIMIZER)
        .unwrap();
    assert_eq!(
        reopened_run
            .candidates
            .iter()
            .map(|candidate| candidate.producer_candidate_id.as_str())
            .collect::<Vec<_>>(),
        run.candidates
            .iter()
            .map(|candidate| candidate.producer_candidate_id.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn sft_optimizer_run_without_candidate_events_has_an_empty_list() {
    let conn = database();
    attach(
        &conn,
        "session_sft",
        MEMBER_OPTIMIZER,
        "opt_sft",
        "2026-08-26T00:00:00Z",
        "SFT",
    )
    .unwrap();
    let group = load_for_session(&conn, "session_sft").unwrap().unwrap();
    let run = group
        .nodes
        .iter()
        .find(|node| node.kind == MEMBER_OPTIMIZER)
        .unwrap();
    assert!(run.candidates.is_empty());
    assert_eq!(run.kind, MEMBER_OPTIMIZER);
}

fn child_request(
    parent_id: &str,
    request_id: &str,
    title: &str,
    relation: Option<&str>,
    created_at: &str,
) -> ExperimentChildCreateRequest {
    ExperimentChildCreateRequest {
        parent_experiment_id: parent_id.to_owned(),
        session_id: None,
        request_id: request_id.to_owned(),
        title: title.to_owned(),
        task: None,
        model: None,
        created_at: created_at.to_owned(),
        relation: relation.map(str::to_owned),
    }
}

#[test]
fn create_child_forked_from_writes_lineage_and_replays_request_id() {
    let conn = database();
    let parent = create(
        &conn,
        ExperimentCreateRequest {
            session_id: "task_fork".into(),
            request_id: "fork-parent".into(),
            title: "Parent".into(),
            task: None,
            model: None,
            created_at: "2026-08-26T00:00:00Z".into(),
        },
    )
    .unwrap();
    let child = create_child(
        &conn,
        child_request(
            &parent.id,
            "fork-req",
            "Fork: Parent",
            Some("forked_from"),
            "2026-08-26T00:01:00Z",
        ),
    )
    .unwrap();
    let parent = get(&conn, &parent.id).unwrap().unwrap();
    assert_eq!(parent.lineage.len(), 1);
    assert_eq!(parent.lineage[0].relation, "forked_from");
    assert_eq!(parent.lineage[0].target_experiment_id, child.id);
    let replay = create_child(
        &conn,
        child_request(
            &parent.id,
            "fork-req",
            "Fork: Parent",
            Some("forked_from"),
            "2026-08-26T00:02:00Z",
        ),
    )
    .unwrap();
    assert_eq!(replay.id, child.id);
}

#[test]
fn create_child_rerun_of_writes_lineage_and_becomes_the_attach_target() {
    let conn = database();
    let parent = create(
        &conn,
        ExperimentCreateRequest {
            session_id: "task_rerun".into(),
            request_id: "rerun-parent".into(),
            title: "Parent".into(),
            task: None,
            model: None,
            created_at: "2026-08-26T00:00:00Z".into(),
        },
    )
    .unwrap();
    let child = create_child(
        &conn,
        child_request(
            &parent.id,
            "rerun-req",
            "Rerun: Parent",
            Some("rerun_of"),
            "2026-08-26T00:01:00Z",
        ),
    )
    .unwrap();
    let parent = get(&conn, &parent.id).unwrap().unwrap();
    assert_eq!(parent.lineage[0].relation, "rerun_of");
    assert_eq!(parent.lineage[0].target_experiment_id, child.id);
    attach(
        &conn,
        "task_rerun",
        MEMBER_OPTIMIZER,
        "opt_rerun",
        "2026-08-26T00:02:00Z",
        "GEPA",
    )
    .unwrap();
    let active = load_for_session(&conn, "task_rerun").unwrap().unwrap();
    assert_eq!(active.id, child.id);
    assert!(active
        .members
        .iter()
        .any(|member| member.member_id == "opt_rerun"));
}

#[test]
fn create_child_unknown_relation_fails_closed() {
    let conn = database();
    let parent = create(
        &conn,
        ExperimentCreateRequest {
            session_id: "task_unknown".into(),
            request_id: "unknown-parent".into(),
            title: "Parent".into(),
            task: None,
            model: None,
            created_at: "2026-08-26T00:00:00Z".into(),
        },
    )
    .unwrap();
    let error = create_child(
        &conn,
        child_request(
            &parent.id,
            "unknown-req",
            "Bad child",
            Some("warm_started_from"),
            "2026-08-26T00:01:00Z",
        ),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("unknown experiment lineage relation"),
        "{error}"
    );
}

#[test]
fn relate_compared_with_between_members_is_idempotent() {
    let conn = database();
    let group = attach(
        &conn,
        "session_cmp",
        MEMBER_OPTIMIZER,
        "opt_a",
        "2026-08-26T00:00:00Z",
        "GEPA A",
    )
    .unwrap();
    attach(
        &conn,
        "session_cmp",
        MEMBER_OPTIMIZER,
        "opt_b",
        "2026-08-26T00:00:01Z",
        "GEPA B",
    )
    .unwrap();
    let source = crate::lineage::store::member_node_id(&group.id, MEMBER_OPTIMIZER, "opt_a");
    let target = crate::lineage::store::member_node_id(&group.id, MEMBER_OPTIMIZER, "opt_b");
    let request = ExperimentRelateRequest {
        experiment_id: group.id.clone(),
        relation: "compared_with".into(),
        source_kind: "member".into(),
        source_id: source.clone(),
        target_kind: "member".into(),
        target_id: target.clone(),
        created_at: "2026-08-26T00:02:00Z".into(),
    };
    let related = relate(&conn, request.clone()).unwrap();
    let compared = related
        .edges
        .iter()
        .filter(|edge| edge.relation == "compared_with")
        .count();
    assert_eq!(compared, 1);
    let replay = relate(&conn, request).unwrap();
    assert_eq!(
        replay
            .edges
            .iter()
            .filter(|edge| edge.relation == "compared_with")
            .count(),
        1
    );
}

#[test]
fn relate_candidates_compare_both_ways_and_promote_survives_reload() {
    let conn = database();
    let group = attach(
        &conn,
        "session_promote",
        MEMBER_OPTIMIZER,
        "opt_promote",
        "2026-08-26T00:00:00Z",
        "GEPA",
    )
    .unwrap();
    upsert_candidate(
        &conn,
        CandidateUpsert {
            optimizer_run_id: "opt_promote".into(),
            producer_candidate_id: "alpha".into(),
            kind: Some("prompt_overlay".into()),
            protocol_id: Some("prompt_overlay.v1".into()),
            status: Some("accepted".into()),
            parent_ids: vec![],
            metrics: None,
            content_digest: None,
            compared_with: None,
            promoted_to: None,
            at: "2026-08-26T00:01:00Z".into(),
        },
    )
    .unwrap();
    upsert_candidate(
        &conn,
        CandidateUpsert {
            optimizer_run_id: "opt_promote".into(),
            producer_candidate_id: "beta".into(),
            kind: Some("prompt_overlay".into()),
            protocol_id: Some("prompt_overlay.v1".into()),
            status: Some("accepted".into()),
            parent_ids: vec![],
            metrics: None,
            content_digest: None,
            compared_with: None,
            promoted_to: None,
            at: "2026-08-26T00:01:01Z".into(),
        },
    )
    .unwrap();
    let alpha = "can:opt_promote:alpha".to_string();
    let beta = "can:opt_promote:beta".to_string();
    relate(
        &conn,
        ExperimentRelateRequest {
            experiment_id: group.id.clone(),
            relation: "compared_with".into(),
            source_kind: "candidate".into(),
            source_id: alpha.clone(),
            target_kind: "candidate".into(),
            target_id: beta.clone(),
            created_at: "2026-08-26T00:02:00Z".into(),
        },
    )
    .unwrap();
    relate(
        &conn,
        ExperimentRelateRequest {
            experiment_id: group.id.clone(),
            relation: "promoted_to".into(),
            source_kind: "candidate".into(),
            source_id: alpha.clone(),
            target_kind: "candidate".into(),
            target_id: beta.clone(),
            created_at: "2026-08-26T00:02:01Z".into(),
        },
    )
    .unwrap();
    upsert_candidate(
        &conn,
        CandidateUpsert {
            optimizer_run_id: "opt_promote".into(),
            producer_candidate_id: "alpha".into(),
            kind: None,
            protocol_id: None,
            status: None,
            parent_ids: vec![],
            metrics: None,
            content_digest: None,
            compared_with: None,
            promoted_to: None,
            at: "2026-08-26T00:03:00Z".into(),
        },
    )
    .unwrap();
    let reloaded = get(&conn, &group.id).unwrap().unwrap();
    let run = reloaded
        .nodes
        .iter()
        .find(|node| node.kind == MEMBER_OPTIMIZER)
        .unwrap();
    let alpha_row = run
        .candidates
        .iter()
        .find(|candidate| candidate.id == alpha)
        .unwrap();
    let beta_row = run
        .candidates
        .iter()
        .find(|candidate| candidate.id == beta)
        .unwrap();
    assert_eq!(alpha_row.compared_with, vec![beta.clone()]);
    assert_eq!(beta_row.compared_with, vec![alpha.clone()]);
    assert_eq!(alpha_row.promoted_to.as_deref(), Some(beta.as_str()));
    assert_eq!(alpha_row.status.as_deref(), Some("promoted"));
}

#[test]
fn relate_mixed_member_and_candidate_fails_closed() {
    let conn = database();
    let group = attach(
        &conn,
        "session_mixed",
        MEMBER_OPTIMIZER,
        "opt_mixed",
        "2026-08-26T00:00:00Z",
        "GEPA",
    )
    .unwrap();
    upsert_candidate(
        &conn,
        CandidateUpsert {
            optimizer_run_id: "opt_mixed".into(),
            producer_candidate_id: "seed".into(),
            kind: Some("prompt_overlay".into()),
            protocol_id: None,
            status: Some("accepted".into()),
            parent_ids: vec![],
            metrics: None,
            content_digest: None,
            compared_with: None,
            promoted_to: None,
            at: "2026-08-26T00:01:00Z".into(),
        },
    )
    .unwrap();
    let node = crate::lineage::store::member_node_id(&group.id, MEMBER_OPTIMIZER, "opt_mixed");
    let error = relate(
        &conn,
        ExperimentRelateRequest {
            experiment_id: group.id,
            relation: "compared_with".into(),
            source_kind: "member".into(),
            source_id: node,
            target_kind: "candidate".into(),
            target_id: "can:opt_mixed:seed".into(),
            created_at: "2026-08-26T00:02:00Z".into(),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("mixed member/candidate"),
        "{error}"
    );
}
