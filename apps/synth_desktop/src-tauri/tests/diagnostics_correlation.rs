//! The acceptance test for the whole system.
//!
//! A container accepts a rollout and then never subscribes its declared stream.
//! An investigator holding only the visual id must get back the stream failure,
//! correlated to its rollout, container, and session, with the upstream cause
//! named and a remediation attached — no CUA, no shell, no raw database.
//!
//! This is acceptance criterion 4 stated as code. If it passes, the reproduced
//! Craftax failure is one typed call away; if it regresses, the system has
//! stopped being worth its own weight.

use serde_json::json;
use std::net::SocketAddr;
use std::time::Duration;
use synth_desktop_lib::container_stream::{wait_for_stream_subscribed, StreamDiagnostics};
use synth_desktop_lib::core_runtime::CoreRuntime;
use synth_desktop_lib::diagnostics::{codes, Correlation, DiagnosticInput, DiagnosticQuery, Severity};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A declared poll authority that heartbeats forever and never subscribes.
async fn spawn_never_subscribing_poll() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let _ = answer_heartbeat(stream).await;
            });
        }
    });
    addr
}

async fn answer_heartbeat(mut stream: TcpStream) -> std::io::Result<()> {
    let mut buffer = [0_u8; 2048];
    let _ = stream.read(&mut buffer).await?;
    // A heartbeat is not a subscribe ACK. This is the exact shape of the
    // failure: the transport is alive and the stream never becomes ready.
    let body = json!({ "events": [{ "kind": "heartbeat", "ready": false }] }).to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await
}

#[tokio::test]
async fn a_stream_that_never_subscribes_is_explained_from_the_visual_id_alone() {
    let dir = tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();
    let addr = spawn_never_subscribing_poll().await;

    // Everything the rollout path knows at the moment it waits for the stream.
    let diagnostics = StreamDiagnostics::new(
        Some(core.diagnostics_service().clone()),
        Correlation {
            container_id: Some("ctr_craftax".into()),
            rollout_id: Some("roll_7".into()),
            visual_id: Some("vis_9".into()),
            visual_revision: Some(14),
            stream_id: Some("stream_7".into()),
            session_id: Some("sess_1".into()),
            ..Default::default()
        },
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    let refused = wait_for_stream_subscribed(
        &client,
        &format!("http://{addr}/poll"),
        Duration::from_millis(300),
        &diagnostics,
    )
    .await;
    assert!(refused.is_err(), "a never-subscribed stream must refuse to start");

    // The renderer notices second, and knows less: only the visual.
    core.diagnostics_service().emit({
        let mut input = DiagnosticInput::new(
            Severity::Error,
            "visual-host",
            "visual.projection.rejected",
            codes::UNSUPPORTED_TRACE_PROJECTION_SCHEMA,
            "Unsupported trace projection schema: synth.trace.v5",
        );
        input.correlation.visual_id = Some("vis_9".into());
        input.correlation.visual_revision = Some(14);
        input
    });

    // One typed call, holding only the visual id.
    let explained = core
        .diagnostics_service()
        .explain(DiagnosticQuery {
            correlation: Correlation {
                visual_id: Some("vis_9".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("explain");

    // The cause is the stream, not the blank pane the user saw.
    assert_eq!(
        explained["cause"]["code"],
        json!(codes::STREAM_SUBSCRIBE_TIMEOUT),
        "explained: {explained:#}"
    );
    assert_eq!(explained["cause"]["component"], json!("container-stream"));
    assert_eq!(
        explained["symptoms"][0]["code"],
        json!(codes::UNSUPPORTED_TRACE_PROJECTION_SCHEMA)
    );

    // Every identity is on the answer, including the ones the caller never had.
    let correlation = &explained["cause"]["correlation"];
    assert_eq!(correlation["rollout_id"], json!("roll_7"));
    assert_eq!(correlation["container_id"], json!("ctr_craftax"));
    assert_eq!(correlation["stream_id"], json!("stream_7"));
    assert_eq!(correlation["session_id"], json!("sess_1"));

    // And a remediation, without a model call.
    let remediation = explained["remediation"].as_str().expect("remediation");
    assert!(remediation.contains("subscribed"), "{remediation}");
    assert_eq!(explained["cause"]["retryable"], json!(true));
}

#[tokio::test]
async fn the_answer_is_the_same_whether_the_index_is_running_or_absent() {
    let dir = tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();
    let addr = spawn_never_subscribing_poll().await;
    let diagnostics = StreamDiagnostics::new(
        Some(core.diagnostics_service().clone()),
        Correlation {
            rollout_id: Some("roll_1".into()),
            visual_id: Some("vis_1".into()),
            ..Default::default()
        },
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let _ = wait_for_stream_subscribed(
        &client,
        &format!("http://{addr}/poll"),
        Duration::from_millis(200),
        &diagnostics,
    )
    .await;

    // No sidecar was ever started in this process, so this is the journal
    // answering. The index is an accelerator, never an authority.
    let result = core
        .diagnostics_service()
        .query(DiagnosticQuery {
            correlation: Correlation {
                visual_id: Some("vis_1".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("query");
    assert_eq!(result["source"], json!("journal"));
    assert_eq!(result["count"], json!(1));
    assert_eq!(
        result["events"][0]["code"],
        json!(codes::STREAM_SUBSCRIBE_TIMEOUT)
    );
}

#[tokio::test]
async fn a_subscribed_stream_records_the_transition_that_bounds_the_search() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buffer = [0_u8; 2048];
                let _ = stream.read(&mut buffer).await;
                let body =
                    json!({ "events": [{ "kind": "stream.subscribed", "ready": true }] }).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });

    let dir = tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();
    let diagnostics = StreamDiagnostics::new(
        Some(core.diagnostics_service().clone()),
        Correlation {
            rollout_id: Some("roll_ok".into()),
            visual_id: Some("vis_ok".into()),
            ..Default::default()
        },
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    wait_for_stream_subscribed(
        &client,
        &format!("http://{addr}/poll"),
        Duration::from_secs(2),
        &diagnostics,
    )
    .await
    .expect("subscribed");

    let result = core
        .diagnostics_service()
        .query(DiagnosticQuery {
            severities: vec![Severity::Info],
            ..Default::default()
        })
        .await
        .expect("query");
    assert_eq!(result["count"], json!(1));
    assert_eq!(result["events"][0]["code"], json!("stream_subscribed"));
    // The wait is what tells an investigator whether the gap opened before or
    // after the stream came up.
    assert!(result["events"][0]["details"]["waited_ms"].is_number());
}

#[tokio::test]
async fn an_uninstrumented_caller_records_nothing_rather_than_reaching_for_a_global() {
    let dir = tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();
    let addr = spawn_never_subscribing_poll().await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let _ = wait_for_stream_subscribed(
        &client,
        &format!("http://{addr}/poll"),
        Duration::from_millis(150),
        &StreamDiagnostics::none(),
    )
    .await;

    let result = core
        .diagnostics_service()
        .query(DiagnosticQuery::default())
        .await
        .expect("query");
    assert_eq!(
        result["count"],
        json!(0),
        "an unattached emitter wrote into another runtime's journal"
    );
}

/// Two runtimes in one process must not see each other's diagnostics. This is
/// the property a process-global emitter handle would quietly destroy.
#[tokio::test]
async fn two_runtimes_in_one_process_stay_isolated() {
    let first_dir = tempdir().unwrap();
    let second_dir = tempdir().unwrap();
    let first = CoreRuntime::open(first_dir.path()).unwrap();
    let second = CoreRuntime::open(second_dir.path()).unwrap();

    first.diagnostics_service().emit(DiagnosticInput::new(
        Severity::Error,
        "container-stream",
        "stream.interrupted",
        codes::STREAM_INTERRUPTED,
        "first runtime only",
    ));

    let one = first
        .diagnostics_service()
        .query(DiagnosticQuery::default())
        .await
        .expect("query");
    let two = second
        .diagnostics_service()
        .query(DiagnosticQuery::default())
        .await
        .expect("query");
    assert_eq!(one["count"], json!(1));
    assert_eq!(two["count"], json!(0));
}
