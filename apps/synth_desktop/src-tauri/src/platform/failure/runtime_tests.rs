//! Isolated failure-runtime invariants: lifecycle, atomic settlement, emergency import.

use crate::platform::failure::{
    definition::FailureDefinition, ContainerFailure, FailureKind, FailureLifecycleState,
    FailureQuery, FailureRuntime, HealthObservation, HealthSource, HealthStatus, TransitionReason,
};
use crate::platform::logging::{emergency_sink, LogRecord, LogRuntime};
use crate::platform::operations::{OperationContext, OperationKind, OperationPhase};
use crate::storage::Storage;
use chrono::Utc;

#[test]
fn container_unhealthy_settles_current_failure_id() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let runtime = FailureRuntime::new(storage.database().clone());
    storage
        .database()
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO containers(id,name,location,status,health_json,metadata_json,created_at,updated_at)
                 VALUES('ctr_1','n','local','ready','{}','{}','now','now')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let failure = storage
        .database()
        .transaction(|conn| {
            crate::domains::containers::raise_probe_failure(
                conn,
                ContainerFailure::Unhealthy {
                    container_id: "ctr_1".into(),
                    observation: HealthObservation {
                        source: HealthSource::LiveProbe,
                        status: HealthStatus::Unhealthy,
                        observed_at: Utc::now(),
                        http_status: Some(503),
                        summary: None,
                    },
                },
                "ctr_1",
                None,
            )
        })
        .unwrap();
    assert_eq!(failure.kind.code(), "container_unhealthy");
    let current: String = storage
        .database()
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT current_failure_id FROM containers WHERE id='ctr_1'",
                [],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(current, failure.failure_id.0);
    let listed = runtime
        .query(FailureQuery {
            container_id: Some("ctr_1".into()),
            ..FailureQuery::default()
        })
        .unwrap();
    assert_eq!(listed.count, 1);
    assert_eq!(listed.failures[0].code, "container_unhealthy");
}

#[test]
fn health_authority_conflict_has_stable_code() {
    let kind = FailureKind::Container(ContainerFailure::HealthAuthorityConflict {
        container_id: "ctr_x".into(),
        registry: HealthObservation {
            source: HealthSource::Registry,
            status: HealthStatus::Ready,
            observed_at: Utc::now(),
            http_status: Some(200),
            summary: None,
        },
        live_probe: HealthObservation {
            source: HealthSource::LiveProbe,
            status: HealthStatus::Unhealthy,
            observed_at: Utc::now(),
            http_status: Some(503),
            summary: None,
        },
    });
    assert_eq!(kind.code(), "container_health_authority_conflict");
    assert!(kind.remediation().is_some());
}

#[test]
fn terminal_lifecycle_cannot_reopen() {
    assert!(crate::platform::failure::lifecycle::allowed(
        FailureLifecycleState::Terminalized,
        FailureLifecycleState::Open
    )
    .is_err());
}

#[test]
fn emergency_import_records_receipt_path() {
    let dir = tempfile::tempdir().unwrap();
    let record = LogRecord::new(
        crate::platform::logging::LogLevel::Error,
        "bootstrap",
        "sqlite_unavailable",
        "simulated sqlite open failure",
    );
    emergency_sink::write_record(dir.path(), &record).unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let logs = LogRuntime::durable(storage.database().clone(), dir.path().to_path_buf());
    let import_id = storage
        .database()
        .transaction(|conn| logs.import_emergency(conn))
        .unwrap();
    assert!(import_id.is_some());
    assert!(!emergency_sink::exists(dir.path()));
}

#[test]
fn admission_maps_evaluator_not_declared() {
    let container = crate::optimizers::admission::ids::ContainerId::new("nanohorizon").unwrap();
    let error = crate::optimizers::admission::AdmissionError::evaluator_not_declared(&container);
    let kind = crate::domains::evaluations::kind_from_admission(&error);
    assert_eq!(kind.code(), "evaluator_not_declared");
}

#[test]
fn raise_records_operation_context() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let runtime = FailureRuntime::new(storage.database().clone());
    let raised = runtime
        .raise(
            FailureKind::Contract(crate::platform::failure::ContractFailure::InvalidEnvelope {
                detail: "no schema".into(),
            }),
            OperationContext::bootstrap("test"),
            OperationKind::Query,
            OperationPhase::Start,
            None,
            "test",
        )
        .unwrap();
    assert_eq!(raised.lifecycle_state, FailureLifecycleState::Open);
    runtime
        .transition(
            raised.failure_id.as_str(),
            FailureLifecycleState::Terminalized,
            TransitionReason::Terminalized,
            "test",
        )
        .unwrap();
}

#[test]
fn admission_failure_marks_unstarted_children_not_started() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    storage
        .database()
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO optimizer_runs(id, algorithm_id, status, source, created_at, payload_json, updated_at)
                 VALUES('run_1', 'eval', 'running', 'local', 'now', '{}', 'now')",
                [],
            )?;
            conn.execute(
                "INSERT INTO evaluation_runs(
                    optimizer_run_id, recipe_source_kind, execution_spec_json, execution_spec_digest,
                    container_declaration_digest, policy_revision, policy_configuration_digest,
                    approval_receipt_id, created_at)
                 VALUES('run_1', 'inline', '{}', 'd', 'c', 'p', 'cfg', 'appr', 'now')",
                [],
            )?;
            conn.execute(
                "INSERT INTO evaluation_rollouts(optimizer_run_id, rollout_index, rollout_state, updated_at)
                 VALUES('run_1', 0, 'queued', 'now')",
                [],
            )?;
            conn.execute(
                "INSERT INTO evaluation_rollouts(optimizer_run_id, rollout_index, rollout_state, updated_at)
                 VALUES('run_1', 1, 'running', 'now')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let container = crate::optimizers::admission::ids::ContainerId::new("nanohorizon").unwrap();
    let error = crate::optimizers::admission::AdmissionError::evaluator_not_declared(&container);
    let raised = storage
        .database()
        .transaction(|conn| crate::domains::evaluations::raise(conn, &error, Some("run_1")))
        .unwrap();
    let states: Vec<(i64, String)> = storage
        .database()
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT rollout_index, rollout_state FROM evaluation_rollouts WHERE optimizer_run_id='run_1' ORDER BY rollout_index",
            )?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(
        states,
        vec![(0, "not_started".into()), (1, "running".into())]
    );
    let terminal: String = storage
        .database()
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT terminal_failure_id FROM evaluation_rollouts WHERE optimizer_run_id='run_1' AND rollout_index=0",
                [],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(terminal, raised.failure_id.0);
}

#[test]
fn historical_error_json_becomes_unclassified() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    storage
        .database()
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO optimizer_runs(id, algorithm_id, status, source, created_at, payload_json, updated_at, error_json)
                 VALUES('run_hist', 'eval', 'failed', 'local', 'now', '{}', 'now', '\"legacy boom\"')",
                [],
            )?;
            crate::platform::failure::repository::migrate_historical_failures(conn)?;
            Ok(())
        })
        .unwrap();
    let listed = FailureRuntime::new(storage.database().clone())
        .query(FailureQuery {
            code: Some("historical_failure_unclassified".into()),
            ..FailureQuery::default()
        })
        .unwrap();
    assert_eq!(listed.count, 1);
    let linked: String = storage
        .database()
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT terminal_failure_id FROM optimizer_runs WHERE id='run_hist'",
                [],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(linked, listed.failures[0].failure_id);
}

#[test]
fn diagnostics_index_degraded_has_stable_code() {
    let kind = FailureKind::Telemetry(crate::platform::failure::TelemetryFailure::IndexDegraded {
        reason: "sidecar unavailable".into(),
    });
    assert_eq!(kind.code(), "diagnostics_index_degraded");
}
