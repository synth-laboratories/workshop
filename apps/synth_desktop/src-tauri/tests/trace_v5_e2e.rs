//! Opt-in end-to-end smoke test against a real local Trace V5 bundle.
//!
//! Run through `scripts/test-trace-v5-real-bundle.sh`; the test is ignored in
//! normal CI because the real Harbor bundle is intentionally not committed.

use std::{env, path::PathBuf};
use synth_desktop_lib::data::DataStore;
use synth_desktop_lib::storage::{ContentStore, Database, Storage};
use synth_desktop_lib::trace_ingest::TraceBundleIngestRequest;
use tempfile::tempdir;

fn real_bundle() -> PathBuf {
    env::var_os("SYNTH_TRACE_V5_REAL_BUNDLE")
        .map(PathBuf::from)
        .expect("SYNTH_TRACE_V5_REAL_BUNDLE must name a real bundle directory or ZIP")
}

#[tokio::test]
#[ignore = "requires a real local bundle and synth-trace; use scripts/test-trace-v5-real-bundle.sh"]
async fn imports_real_bundle_into_trusted_catalog_and_keeps_duplicate_identity() {
    let source = real_bundle();
    assert!(
        source.exists(),
        "real bundle does not exist: {}",
        source.display()
    );

    let root = tempdir().unwrap();
    let storage = Storage::open(root.path()).unwrap();
    let data = DataStore::new(
        storage.database().clone(),
        ContentStore::new(storage.content_root()),
    );
    let request = TraceBundleIngestRequest {
        source_path: source.display().to_string(),
        source_kind: Some("real_harbor_smoke".into()),
        title: Some("Real Trace V5 smoke".into()),
        source_uri: Some("dogfood://harbor/trace-v5".into()),
        container_id: None,
    };

    let (first, first_event) = data
        .ingest_trace_bundle(request.clone())
        .await
        .expect("first real-bundle import");
    assert!(first.trusted);
    assert_eq!(first.compatibility_level, "native");
    assert!(first
        .bundle_digest
        .as_deref()
        .is_some_and(|v| v.starts_with("sha256:")));
    assert!(first
        .archive_digest
        .as_deref()
        .is_some_and(|v| v.starts_with("sha256:")));
    assert!(!first.traces.is_empty());
    assert!(!first.duplicate);
    assert_eq!(
        first_event.as_ref().map(|event| event.kind.as_str()),
        Some("trace.bundle.imported")
    );

    let listed = data.list_traces().await.unwrap();
    assert_eq!(listed.len(), first.traces.len());
    let expected = &first.traces[0];
    let by_id = data.get_trace(expected.id.clone()).await.unwrap();
    let by_digest = data.get_trace(expected.digest.clone()).await.unwrap();
    assert_eq!(by_id, by_digest);
    assert_eq!(by_id.title, "Real Trace V5 smoke");

    let archive_path = PathBuf::from(
        by_id
            .path
            .as_deref()
            .expect("trusted trace row must point at its CAS archive"),
    );
    assert!(archive_path.is_file());
    assert!(archive_path.starts_with(storage.content_root().join("traces")));

    let projection = data
        .resolve_trace_projection(by_id.digest.clone(), "rollout-inspector".into())
        .await
        .expect("resolve viewer packet from the trusted archive");
    assert_eq!(projection.trace_digest, by_id.digest);
    assert_eq!(projection.projection_kind, "rollout-inspector");
    assert_eq!(
        projection.projection_schema,
        "synth.trace-projection.rollout-inspector.v1"
    );
    assert_eq!(
        projection.payload["schema_version"],
        "synth.trace-projection.rollout-inspector.v1"
    );
    assert!(projection.payload["visual"]["items"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let counts_before = catalog_counts(storage.database());
    let (second, second_event) = data
        .ingest_trace_bundle(request)
        .await
        .expect("duplicate real-bundle import");
    assert_eq!(second.input_digest, first.input_digest);
    assert_eq!(second.bundle_digest, first.bundle_digest);
    assert_eq!(second.archive_digest, first.archive_digest);
    assert_eq!(second.traces, first.traces);
    assert!(second.duplicate);
    assert!(second_event.is_none());
    assert_eq!(catalog_counts(storage.database()), counts_before);
}

fn catalog_counts(db: &std::sync::Arc<Database>) -> (i64, i64, i64, i64) {
    db.with_conn(|conn| {
        Ok((
            conn.query_row("SELECT COUNT(*) FROM trace_imports", [], |row| row.get(0))?,
            conn.query_row("SELECT COUNT(*) FROM trace_bundles", [], |row| row.get(0))?,
            conn.query_row("SELECT COUNT(*) FROM trace_bundle_members", [], |row| {
                row.get(0)
            })?,
            conn.query_row("SELECT COUNT(*) FROM traces", [], |row| row.get(0))?,
        ))
    })
    .unwrap()
}
