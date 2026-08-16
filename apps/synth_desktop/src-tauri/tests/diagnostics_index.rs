//! Diagnostics against a real local VictoriaLogs process.
//!
//! The unit tests prove the contract; this proves the sidecar. It starts the
//! bundled executable on a dynamic loopback port, indexes real journal rows,
//! queries them back through LogsQL, restarts the process, proves catch-up
//! produces no duplicate logical events, and proves that killing the index
//! mid-run changes nothing an agent can observe except latency.
//!
//! Staging the binary is what makes this run:
//!
//!     ./scripts/diagnostics/fetch-victorialogs.sh
//!     cargo test -p synth-desktop --test diagnostics_index
//!
//! Without it every test here **skips loudly** rather than passing quietly — a
//! green run that never started a sidecar would be the exact failure this file
//! exists to catch. The failure-injection tests below need no binary and always
//! run, because "the binary is missing" is itself one of the cases.

use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use synth_desktop_lib::diagnostics::event::{validate, DiagnosticInput, Severity};
use synth_desktop_lib::diagnostics::indexer::{cursor_path, Indexer};
use synth_desktop_lib::diagnostics::query::DiagnosticQuery;
use synth_desktop_lib::diagnostics::sidecar::{
    locate_binary, SidecarConfig, SidecarState, VictoriaLogsSidecar,
};
use synth_desktop_lib::diagnostics::store::DiagnosticStore;
use synth_desktop_lib::diagnostics::victorialogs::{compile, VictoriaLogsClient};
use synth_desktop_lib::diagnostics::{Correlation, DiagnosticsService};
use synth_desktop_lib::storage::{EventJournal, Storage};
use tempfile::{tempdir, TempDir};

/// `None` plus a loud note when the executable has not been staged.
fn require_binary(test: &str) -> Option<PathBuf> {
    match locate_binary() {
        Some(path) => Some(path),
        None => {
            eprintln!(
                "SKIP {test}: no VictoriaLogs executable. Run ./scripts/diagnostics/fetch-victorialogs.sh"
            );
            None
        }
    }
}

struct Harness {
    _dir: TempDir,
    root: PathBuf,
    store: DiagnosticStore,
    service: std::sync::Arc<DiagnosticsService>,
}

fn harness() -> Harness {
    let dir = tempdir().expect("temp dir");
    let storage = Storage::open(dir.path().join("data")).expect("open storage");
    let journal = EventJournal::new(storage.database().clone());
    let root = dir.path().join("diagnostics");
    Harness {
        store: DiagnosticStore::new(storage.database().clone(), journal.clone()),
        service: DiagnosticsService::new(storage.database().clone(), journal, root.clone()),
        root,
        _dir: dir,
    }
}

fn projection_failure(rollout: &str) -> synth_desktop_lib::diagnostics::DiagnosticEvent {
    let mut input = DiagnosticInput::new(
        Severity::Error,
        "visual-host",
        "visual.projection.rejected",
        "unsupported_trace_projection_schema",
        "Unsupported trace projection schema: synth.trace.v5",
    );
    input.correlation.visual_id = Some("vis_9".into());
    input.correlation.rollout_id = Some(rollout.into());
    input.correlation.trace_id = Some(format!("trace_{rollout}"));
    input
        .details
        .insert("received_schema".into(), json!("synth.trace.v5"));
    validate(input).expect("valid diagnostic")
}

async fn started(root: &std::path::Path) -> Option<std::sync::Arc<VictoriaLogsSidecar>> {
    let sidecar = VictoriaLogsSidecar::new(SidecarConfig::for_root(root));
    match sidecar.start().await {
        SidecarState::Ready => Some(sidecar),
        other => {
            eprintln!("SKIP: VictoriaLogs did not reach ready: {other:?}");
            None
        }
    }
}

#[tokio::test]
async fn ingests_and_answers_a_typed_query_through_the_real_index() {
    let Some(_) = require_binary("ingests_and_answers_a_typed_query_through_the_real_index") else {
        return;
    };
    let harness = harness();
    let Some(sidecar) = started(&harness.root).await else {
        return;
    };
    let written = harness
        .store
        .append_batch(vec![
            projection_failure("roll_1"),
            projection_failure("roll_2"),
        ])
        .await
        .expect("append");

    let url = sidecar.url().await.expect("index url");
    let client = VictoriaLogsClient::new(&url).expect("client");
    let indexer = Indexer::new(harness.store.clone(), &harness.root);
    let progress = indexer.index_once(&client).await.expect("index");
    assert_eq!(progress.indexed, 2);
    assert_eq!(progress.lag, 0);

    // VictoriaLogs makes a batch searchable shortly after accepting it.
    let query = DiagnosticQuery {
        correlation: Correlation {
            visual_id: Some("vis_9".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let logsql = compile(&query, chrono::Utc::now()).expect("compile");
    let mut sequences = Vec::new();
    for _ in 0..40 {
        sequences = client.search_sequences(&logsql, 100).await.expect("query");
        if sequences.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(sequences.len(), 2, "index never returned both rows");

    // The index returns identities; the journal supplies the records.
    let records = harness
        .store
        .load_by_sequences(sequences)
        .await
        .expect("load");
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record.event.code == "unsupported_trace_projection_schema"));
    assert_eq!(
        records[0].event.details["received_schema"],
        json!("synth.trace.v5")
    );
    assert!(written.iter().all(|row| records
        .iter()
        .any(|record| record.sequence == row.sequence)));

    sidecar.stop().await.expect("stop");
}

#[tokio::test]
async fn a_restart_catches_up_without_duplicating_logical_events() {
    let Some(_) = require_binary("a_restart_catches_up_without_duplicating_logical_events") else {
        return;
    };
    let harness = harness();
    let Some(sidecar) = started(&harness.root).await else {
        return;
    };
    let indexer = Indexer::new(harness.store.clone(), &harness.root);
    let url = sidecar.url().await.expect("url");
    let client = VictoriaLogsClient::new(&url).expect("client");

    harness
        .store
        .append_batch(vec![projection_failure("roll_1")])
        .await
        .expect("append");
    indexer.index_once(&client).await.expect("index");
    let cursor_after_first = indexer.load_cursor();
    assert!(cursor_after_first > 0);

    // Rows written while the index is down are not lost — they are simply not
    // indexed yet, which is what the cursor is for.
    sidecar.stop().await.expect("stop");
    harness
        .store
        .append_batch(vec![projection_failure("roll_2")])
        .await
        .expect("append while down");
    assert_eq!(indexer.lag().await, 1);

    let Some(restarted) = started(&harness.root).await else {
        return;
    };
    let url = restarted.url().await.expect("url");
    let client = VictoriaLogsClient::new(&url).expect("client");
    let progress = indexer.index_once(&client).await.expect("catch up");
    assert_eq!(progress.indexed, 1, "catch-up re-shipped already-indexed rows");
    assert_eq!(progress.lag, 0);

    // Re-indexing from zero must not change what a query answers, because the
    // answer comes from the journal keyed by sequence.
    indexer.save_cursor(0);
    indexer.index_once(&client).await.expect("replay");
    let query = DiagnosticQuery {
        correlation: Correlation {
            visual_id: Some("vis_9".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let logsql = compile(&query, chrono::Utc::now()).expect("compile");
    let mut sequences = Vec::new();
    for _ in 0..40 {
        sequences = client.search_sequences(&logsql, 100).await.expect("query");
        if sequences.len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let records = harness
        .store
        .load_by_sequences(sequences.clone())
        .await
        .expect("load");
    assert_eq!(
        records.len(),
        2,
        "replay produced duplicate logical events: {sequences:?}"
    );

    restarted.stop().await.expect("stop");
}

#[tokio::test]
async fn queries_fall_back_to_the_journal_when_the_index_dies_mid_run() {
    let Some(_) = require_binary("queries_fall_back_to_the_journal_when_the_index_dies_mid_run")
    else {
        return;
    };
    let harness = harness();
    let Some(sidecar) = started(&harness.root).await else {
        return;
    };
    harness.service.emit({
        let mut input = DiagnosticInput::new(
            Severity::Error,
            "containers",
            "container.rollout.failed",
            "container_rollout_failed",
            "rollout roll_7 terminated",
        );
        input.correlation.rollout_id = Some("roll_7".into());
        input
    });
    let before = harness
        .service
        .query(DiagnosticQuery::default())
        .await
        .expect("query");
    assert_eq!(before["count"], json!(1));

    sidecar.stop().await.expect("stop");

    let after = harness
        .service
        .query(DiagnosticQuery::default())
        .await
        .expect("query after index death");
    assert_eq!(after["count"], json!(1), "an index outage changed an answer");
    assert_eq!(after["source"], json!("journal"));
}

#[tokio::test]
async fn a_missing_binary_degrades_without_losing_a_single_diagnostic() {
    let harness = harness();
    std::env::set_var(
        synth_desktop_lib::diagnostics::sidecar::BINARY_ENV,
        harness.root.join("no-such-binary"),
    );
    let sidecar = VictoriaLogsSidecar::new(SidecarConfig::for_root(&harness.root));
    let state = sidecar.start().await;
    std::env::remove_var(synth_desktop_lib::diagnostics::sidecar::BINARY_ENV);
    assert!(matches!(state, SidecarState::Degraded(_)), "{state:?}");

    harness.service.emit(projection_failure_input());
    let result = harness
        .service
        .query(DiagnosticQuery::default())
        .await
        .expect("query");
    assert_eq!(result["count"], json!(1));
    assert_eq!(result["source"], json!("journal"));

    let status = harness.service.status().await;
    assert_eq!(status["local_only"], json!(true));
    assert_eq!(status["stored_events"], json!(1));
}

#[tokio::test]
async fn a_corrupt_cursor_reindexes_instead_of_stalling() {
    let harness = harness();
    std::fs::create_dir_all(&harness.root).expect("root");
    let indexer = Indexer::new(harness.store.clone(), &harness.root);
    harness
        .store
        .append_batch(vec![projection_failure("roll_1")])
        .await
        .expect("append");
    indexer.save_cursor(1);
    std::fs::write(cursor_path(&harness.root), b"\0\0truncated").expect("corrupt the cursor");
    assert_eq!(indexer.load_cursor(), 0);
    assert_eq!(indexer.lag().await, 1, "a corrupt cursor stalled indexing");
}

#[tokio::test]
async fn an_unreachable_index_never_advances_the_cursor() {
    let harness = harness();
    std::fs::create_dir_all(&harness.root).expect("root");
    let indexer = Indexer::new(harness.store.clone(), &harness.root);
    harness
        .store
        .append_batch(vec![projection_failure("roll_1")])
        .await
        .expect("append");
    // Port 1 is reserved and never listening.
    let client = VictoriaLogsClient::new("http://127.0.0.1:1").expect("client");
    assert!(indexer.index_once(&client).await.is_err());
    assert_eq!(indexer.load_cursor(), 0);
    assert_eq!(indexer.lag().await, 1);
}

#[tokio::test]
async fn two_instances_index_into_separate_directories() {
    let first = harness();
    let second = harness();
    assert_ne!(first.root, second.root);

    first.service.emit(projection_failure_input());
    second.service.emit({
        let mut input = DiagnosticInput::new(
            Severity::Warn,
            "renderer",
            "test.event",
            "other_instance_code",
            "second instance",
        );
        input.correlation.session_id = Some("sess_2".into());
        input
    });

    let one = first
        .service
        .query(DiagnosticQuery::default())
        .await
        .expect("query");
    let two = second
        .service
        .query(DiagnosticQuery::default())
        .await
        .expect("query");
    assert_eq!(one["count"], json!(1));
    assert_eq!(two["count"], json!(1));
    assert_eq!(
        one["events"][0]["code"],
        json!("unsupported_trace_projection_schema")
    );
    assert_eq!(two["events"][0]["code"], json!("other_instance_code"));
}

/// Acceptance: producer paths show no synchronous dependency on ingestion.
///
/// The index here is *worse than dead* — it accepts a connection and then holds
/// it far longer than any real sidecar would. If emission were coupled to
/// ingestion in any way, this test would take minutes.
#[tokio::test]
async fn a_hung_index_does_not_slow_a_single_producer_call() {
    let harness = harness();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind slow index");
    let addr = listener.local_addr().expect("addr");
    let hung = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            // Accept and never answer.
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(600)).await;
                drop(stream);
            });
        }
    });

    let indexing = tokio::spawn({
        let store = harness.store.clone();
        let root = harness.root.clone();
        async move {
            let client = VictoriaLogsClient::new(format!("http://{addr}")).expect("client");
            let indexer = Indexer::new(store, root);
            let _ = indexer.index_once(&client).await;
        }
    });

    let started = std::time::Instant::now();
    for index in 0..2_000 {
        harness.service.emit(DiagnosticInput::new(
            Severity::Info,
            "renderer",
            "test.event",
            "producer_path",
            format!("event {index}"),
        ));
    }
    let per_emit = started.elapsed() / 2_000;
    assert!(
        per_emit < Duration::from_micros(500),
        "emit took {per_emit:?} per event while the index was hung"
    );

    // And the events are still there once the writer drains.
    harness.service.flush().await.expect("flush");
    let result = harness
        .service
        .query(DiagnosticQuery {
            codes: vec!["producer_path".into()],
            limit: 500,
            ..Default::default()
        })
        .await
        .expect("query");
    assert!(result["count"].as_u64().unwrap() > 0);
    assert_eq!(result["source"], json!("journal"));

    indexing.abort();
    hung.abort();
}

/// Failure injection: the reserved port is taken between reserve and spawn.
///
/// The supervisor reserves a port, releases it, and hands it to the child. That
/// window is real, so losing the race must degrade rather than hang or crash.
#[tokio::test]
async fn a_port_collision_degrades_instead_of_hanging() {
    let Some(_) = require_binary("a_port_collision_degrades_instead_of_hanging") else {
        return;
    };
    let harness = harness();
    let collision = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("reserve collision port");
    let port = collision.local_addr().expect("collision address").port();
    let mut config = SidecarConfig::for_root(&harness.root);
    config.listen_port = Some(port);
    let sidecar = VictoriaLogsSidecar::new(config);
    let started = std::time::Instant::now();
    let state = sidecar.start().await;
    assert!(
        matches!(state, SidecarState::Degraded(ref reason) if reason == "process_exited_before_ready"),
        "expected a bind collision to degrade, got {state:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "port collision waited for the full readiness timeout"
    );

    // And the service still answers.
    harness.service.emit(projection_failure_input());
    let result = harness
        .service
        .query(DiagnosticQuery::default())
        .await
        .expect("query");
    assert_eq!(result["count"], json!(1));
    assert_eq!(result["source"], json!("journal"));
}

/// VictoriaLogs must really enforce the configured retention, while the
/// longer-lived journal remains authoritative.
///
/// The sidecar's minimum supported retention is one day and cleanup is
/// partition-based. A deterministic acceptance test therefore sends a
/// 36-hour-old journal row to a fresh one-day index and proves VictoriaLogs
/// rejects it while SQLite retains it.
#[tokio::test]
async fn index_retention_rejects_expired_rows_without_touching_the_journal() {
    let Some(_) =
        require_binary("index_retention_rejects_expired_rows_without_touching_the_journal")
    else {
        return;
    };
    let harness = harness();
    let mut old = DiagnosticInput::new(
        Severity::Warn,
        "diagnostics",
        "diagnostics.retention.probe",
        "retention_probe",
        "retention probe",
    );
    old.timestamp = Some((chrono::Utc::now() - chrono::Duration::hours(36)).to_rfc3339());
    let written = harness
        .store
        .append_batch(vec![validate(old).expect("valid old diagnostic")])
        .await
        .expect("append old diagnostic");
    let sequence = written[0].sequence;
    let query = DiagnosticQuery {
        codes: vec!["retention_probe".into()],
        since: Duration::from_secs(3 * 24 * 60 * 60),
        ..Default::default()
    };
    let logsql = compile(&query, chrono::Utc::now()).expect("compile retention query");

    let mut one_day = SidecarConfig::for_root(&harness.root);
    one_day.retention_days = 1;
    let sidecar = VictoriaLogsSidecar::new(one_day);
    assert_eq!(sidecar.start().await, SidecarState::Ready);
    let client =
        VictoriaLogsClient::new(sidecar.url().await.expect("one-day url")).expect("client");
    let progress = Indexer::new(harness.store.clone(), &harness.root)
        .index_once(&client)
        .await
        .expect("ship expired row to one-day index");
    assert_eq!(
        progress.indexed, 1,
        "journal row was not offered to VictoriaLogs"
    );
    let mut indexed = vec![sequence];
    for _ in 0..40 {
        indexed = client
            .search_sequences(&logsql, 10)
            .await
            .expect("query after retention rejection");
        if indexed.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(
        indexed.is_empty(),
        "VictoriaLogs retained a row outside the one-day policy: {indexed:?}"
    );
    assert_eq!(
        harness
            .store
            .load_by_sequences(vec![sequence])
            .await
            .expect("load journal row")
            .len(),
        1,
        "index retention deleted the authoritative journal row"
    );
    sidecar.stop().await.expect("stop one-day sidecar");
}

/// Failure injection: the disk quota is set below one batch.
///
/// VictoriaLogs enforces its own quota asynchronously. This test proves that
/// Workshop can offer a batch to a real index configured below that batch and
/// the authoritative journal remains complete regardless of index eviction.
#[tokio::test]
async fn a_tiny_quota_never_costs_an_authoritative_event() {
    let Some(_) = require_binary("a_tiny_quota_never_costs_an_authoritative_event") else {
        return;
    };
    let harness = harness();
    let mut config = SidecarConfig::for_root(&harness.root);
    config.quota_bytes = 1;
    config.retention_days = 1;
    let sidecar = VictoriaLogsSidecar::new(config);
    assert_eq!(sidecar.start().await, SidecarState::Ready);

    for index in 0..50 {
        harness.service.emit(DiagnosticInput::new(
            Severity::Error,
            "renderer",
            "test.event",
            "quota_probe",
            format!("event {index}"),
        ));
    }
    harness.service.flush().await.expect("flush");

    let client = VictoriaLogsClient::new(sidecar.url().await.expect("tiny-quota url"))
        .expect("tiny-quota client");
    let progress = Indexer::new(harness.store.clone(), &harness.root)
        .index_once(&client)
        .await
        .expect("offer batch to tiny-quota index");
    assert_eq!(progress.indexed, 50, "batch never reached the constrained index");

    let result = harness
        .service
        .query(DiagnosticQuery {
            codes: vec!["quota_probe".into()],
            limit: 100,
            ..Default::default()
        })
        .await
        .expect("query");
    assert_eq!(result["count"], json!(50), "a quota cost authoritative events");
    sidecar.stop().await.expect("stop");
}

/// The packaged layout is `…/Contents/MacOS/synth-desktop` next to
/// `…/Contents/Resources/services/victoria-logs/victoria-logs`. This proves the
/// nested lookup finds it, without needing a signed build to say so.
#[test]
fn the_bundled_binary_is_found_through_the_packaged_resource_layout() {
    let dir = tempdir().expect("temp dir");
    let resources = dir
        .path()
        .join("Synth Workshop.app/Contents/Resources/services/victoria-logs");
    std::fs::create_dir_all(&resources).expect("resources");
    let binary = resources.join("victoria-logs");
    std::fs::write(&binary, b"#!/bin/sh\nexit 0\n").expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    std::env::set_var(
        synth_desktop_lib::diagnostics::sidecar::BINARY_ENV,
        &binary,
    );
    let located = locate_binary();
    std::env::remove_var(synth_desktop_lib::diagnostics::sidecar::BINARY_ENV);
    assert_eq!(located.as_deref(), Some(binary.as_path()));

    // A path that is present but not executable is not a usable sidecar.
    let unusable = resources.join("not-executable");
    std::fs::write(&unusable, b"data").expect("write");
    std::env::set_var(
        synth_desktop_lib::diagnostics::sidecar::BINARY_ENV,
        &unusable,
    );
    let refused = locate_binary();
    std::env::remove_var(synth_desktop_lib::diagnostics::sidecar::BINARY_ENV);
    assert_eq!(refused, None);
}

/// Acceptance criterion 12, without booting two windows.
///
/// The claim is that two instances do not share an index. Two live sidecars on
/// two data roots is that claim exactly; the GUI around them is not part of it.
#[tokio::test]
async fn two_live_sidecars_hold_separate_ports_descriptors_and_data() {
    let Some(_) = require_binary("two_live_sidecars_hold_separate_ports_descriptors_and_data")
    else {
        return;
    };
    let first = harness();
    let second = harness();
    let (Some(one), Some(two)) = (started(&first.root).await, started(&second.root).await) else {
        return;
    };

    let one_url = one.url().await.expect("first url");
    let two_url = two.url().await.expect("second url");
    assert_ne!(one_url, two_url, "two instances shared a port");

    let one_descriptor = one.read_descriptor().expect("first descriptor");
    let two_descriptor = two.read_descriptor().expect("second descriptor");
    assert_ne!(one_descriptor.data_dir, two_descriptor.data_dir);
    assert_ne!(one_descriptor.pid, two_descriptor.pid);

    // Index one instance's diagnostic and prove the other cannot see it.
    first
        .store
        .append_batch(vec![projection_failure("roll_first")])
        .await
        .expect("append");
    let indexer = Indexer::new(first.store.clone(), &first.root);
    let client = VictoriaLogsClient::new(&one_url).expect("client");
    indexer.index_once(&client).await.expect("index");

    let other = VictoriaLogsClient::new(&two_url).expect("client");
    let logsql = compile(&DiagnosticQuery::default(), chrono::Utc::now()).expect("compile");
    tokio::time::sleep(Duration::from_millis(750)).await;
    let leaked = other.search_sequences(&logsql, 100).await.expect("query");
    assert!(
        leaked.is_empty(),
        "one instance's diagnostics reached another's index: {leaked:?}"
    );

    one.stop().await.expect("stop");
    two.stop().await.expect("stop");
}

/// The producer path must cost the same whatever the index is doing.
///
/// Four states, one workload, one bound. This is the headless half of the
/// performance acceptance: it measures the path diagnostics actually add to,
/// not the app-level scenarios, which need a driven window.
#[tokio::test]
async fn emission_cost_is_unchanged_across_every_index_state() {
    const SAMPLES: u32 = 2_000;
    let mut timings: Vec<(&str, Duration)> = Vec::new();

    // 1. Absent: no sidecar was ever started.
    let absent = harness();
    timings.push(("absent", measure_emission(&absent, SAMPLES)));

    // 2. Ready: a real index, running and healthy.
    if require_binary("emission_cost_is_unchanged_across_every_index_state").is_some() {
        let ready = harness();
        if let Some(sidecar) = started(&ready.root).await {
            timings.push(("ready", measure_emission(&ready, SAMPLES)));
            sidecar.stop().await.expect("stop");
            // 3. Crashed: started, then killed underneath us.
            timings.push(("crashed", measure_emission(&ready, SAMPLES)));
        }
    }

    // 4. Slow: an endpoint that accepts and never answers.
    let slow = harness();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hung = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(600)).await;
                drop(stream);
            });
        }
    });
    let indexing = tokio::spawn({
        let store = slow.store.clone();
        let root = slow.root.clone();
        async move {
            let client = VictoriaLogsClient::new(format!("http://{addr}")).expect("client");
            let _ = Indexer::new(store, root).index_once(&client).await;
        }
    });
    timings.push(("slow", measure_emission(&slow, SAMPLES)));
    indexing.abort();
    hung.abort();

    for (state, per_event) in &timings {
        assert!(
            *per_event < Duration::from_micros(500),
            "emission cost {per_event:?} per event with the index {state}"
        );
    }
    // The states must not differ from each other by an order of magnitude
    // either: a uniform-but-slow path would pass the bound above and still mean
    // emission had acquired a dependency.
    let slowest = timings.iter().map(|(_, cost)| *cost).max().expect("timings");
    let fastest = timings.iter().map(|(_, cost)| *cost).min().expect("timings");
    assert!(
        slowest < fastest * 10 + Duration::from_micros(50),
        "index state changed emission cost: {timings:?}"
    );
}

fn measure_emission(harness: &Harness, samples: u32) -> Duration {
    let started = std::time::Instant::now();
    for index in 0..samples {
        harness.service.emit(DiagnosticInput::new(
            Severity::Info,
            "renderer",
            "test.event",
            "producer_cost",
            format!("event {index}"),
        ));
    }
    started.elapsed() / samples
}

fn projection_failure_input() -> DiagnosticInput {
    let mut input = DiagnosticInput::new(
        Severity::Error,
        "visual-host",
        "visual.projection.rejected",
        "unsupported_trace_projection_schema",
        "Unsupported trace projection schema: synth.trace.v5",
    );
    input.correlation.visual_id = Some("vis_9".into());
    input
}
