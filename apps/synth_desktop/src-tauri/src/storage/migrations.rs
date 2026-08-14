use anyhow::{Context, Result};
use rusqlite::Connection;

/// Every embedded migration, in order. Version N applies only after 1..=N-1.
const MIGRATIONS: &[&str] = &[
    MIGRATION_1,
    MIGRATION_2,
    MIGRATION_3,
    MIGRATION_4,
    MIGRATION_5,
    MIGRATION_6,
    MIGRATION_7,
    MIGRATION_8,
    MIGRATION_9,
    MIGRATION_10,
    MIGRATION_11,
    MIGRATION_12,
    MIGRATION_13,
    MIGRATION_14,
];

/// Apply every migration the database has not reached yet.
pub fn apply_migrations(conn: &Connection) -> Result<i64> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;
    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for (index, migration) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if current < version {
            apply_one(conn, version, migration)?;
        }
    }

    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if version >= 12 {
        fold_rollback_usage_rows(conn)?;
    }
    Ok(version)
}

/// Migration 11 keeps an empty legacy table as a rollback write buffer. A v10
/// binary can therefore launch against a v11 database and keep recording local
/// dev charges. The next current-binary launch folds those rows into the authoritative
/// ledger and clears the buffer atomically, so neither binary double-counts.
fn fold_rollback_usage_rows(conn: &Connection) -> Result<()> {
    let batch = format!("BEGIN IMMEDIATE;\n{FOLD_LEGACY_USAGE_ROWS}\nCOMMIT;");
    if let Err(error) = conn.execute_batch(&batch) {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(anyhow::Error::from(error).context("fold rollback usage rows"));
    }
    Ok(())
}

/// One migration and its version stamp commit as a single transaction: a
/// crash leaves the database entirely before or entirely after a version,
/// never between. Half-applying a data-moving migration (6 renames a table,
/// 8 copies one) while the stamp stays behind would make every relaunch
/// replay the move into already-moved data and wedge the app at open.
fn apply_one(conn: &Connection, version: i64, sql: &str) -> Result<()> {
    let batch = format!(
        "BEGIN IMMEDIATE;\n{sql}\nINSERT INTO schema_migrations(version, applied_at) VALUES ({version}, datetime('now'));\nCOMMIT;"
    );
    if let Err(error) = conn.execute_batch(&batch) {
        // A failed batch can leave its transaction open on the shared
        // connection; roll it back so the caller's error path still works.
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(anyhow::Error::from(error).context(format!("apply schema migration {version}")));
    }
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )
    .context("read schema version")
}

const MIGRATION_1: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    vcs TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    target_json TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    remote_id TEXT,
    codex_thread_id TEXT,
    status TEXT NOT NULL,
    state_generation INTEGER,
    latest_cursor INTEGER NOT NULL DEFAULT 0,
    active_run_id TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    latest_cursor INTEGER NOT NULL DEFAULT 0,
    checkpoint_json TEXT,
    outcome_json TEXT,
    model TEXT,
    adapter TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT
);

CREATE TABLE IF NOT EXISTS events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    session_sequence INTEGER,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    remote_sequence INTEGER,
    command_id TEXT,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS events_session_sequence
ON events(session_id, session_sequence)
WHERE session_id IS NOT NULL AND session_sequence IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS events_remote_dedupe
ON events(session_id, source, remote_sequence)
WHERE remote_sequence IS NOT NULL;

CREATE INDEX IF NOT EXISTS events_global_sequence ON events(sequence);
CREATE INDEX IF NOT EXISTS sessions_updated_at ON sessions(updated_at DESC);

CREATE TABLE IF NOT EXISTS source_cursors (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    cursor INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, source)
);

CREATE TABLE IF NOT EXISTS approvals (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    request_method TEXT NOT NULL,
    status TEXT NOT NULL,
    request_json TEXT NOT NULL,
    decision TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE TABLE IF NOT EXISTS containers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    location TEXT NOT NULL,
    status TEXT NOT NULL,
    base_url TEXT,
    pool_id TEXT,
    task_family TEXT,
    last_rollout_id TEXT,
    health_json TEXT NOT NULL DEFAULT '{}',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS traces (
    id TEXT PRIMARY KEY,
    digest TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    source TEXT NOT NULL,
    container_id TEXT REFERENCES containers(id) ON DELETE SET NULL,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    reward REAL,
    metrics_json TEXT NOT NULL DEFAULT '[]',
    path TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS visuals (
    id TEXT PRIMARY KEY,
    current_revision INTEGER NOT NULL DEFAULT 1,
    title TEXT NOT NULL,
    template_id TEXT NOT NULL,
    status TEXT NOT NULL,
    renderer_kind TEXT NOT NULL,
    bindings_json TEXT NOT NULL DEFAULT '{}',
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    message_id TEXT,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    trace_id TEXT REFERENCES traces(id) ON DELETE SET NULL,
    parent_visual_id TEXT REFERENCES visuals(id) ON DELETE SET NULL,
    source_agent_id TEXT,
    source_model TEXT,
    content_digest TEXT,
    preview_digest TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS visual_revisions (
    visual_id TEXT NOT NULL REFERENCES visuals(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    template_id TEXT NOT NULL,
    renderer_kind TEXT NOT NULL,
    content_digest TEXT,
    bindings_digest TEXT,
    bindings_json TEXT,
    preview_digest TEXT,
    author_agent_id TEXT,
    parent_revision INTEGER,
    created_at TEXT NOT NULL,
    PRIMARY KEY (visual_id, revision)
);

CREATE TABLE IF NOT EXISTS visual_relationships (
    visual_id TEXT NOT NULL REFERENCES visuals(id) ON DELETE CASCADE,
    related_kind TEXT NOT NULL,
    related_id TEXT NOT NULL,
    PRIMARY KEY (visual_id, related_kind, related_id)
);

CREATE TABLE IF NOT EXISTS usage_ledger (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runtime_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

const MIGRATION_2: &str = r#"
ALTER TABLE runs ADD COLUMN updated_at TEXT;
UPDATE runs
SET updated_at = COALESCE(completed_at, started_at, created_at)
WHERE updated_at IS NULL;

CREATE INDEX IF NOT EXISTS runs_session_created
ON runs(session_id, created_at DESC);

CREATE TABLE IF NOT EXISTS command_receipts (
    command_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    request_json TEXT NOT NULL DEFAULT '{}',
    response_json TEXT,
    remote_cursor INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS command_receipts_session_created
ON command_receipts(session_id, created_at DESC);
"#;

const MIGRATION_3: &str = r#"
CREATE TABLE IF NOT EXISTS trace_imports (
    input_digest TEXT PRIMARY KEY,
    stored_path TEXT,
    source_kind TEXT NOT NULL,
    source_uri TEXT,
    compatibility_level TEXT NOT NULL,
    validation_status TEXT NOT NULL,
    detected_schema TEXT,
    detected_bundle_digest TEXT,
    byte_size INTEGER NOT NULL DEFAULT 0,
    imported_at TEXT NOT NULL,
    error_json TEXT NOT NULL DEFAULT '[]',
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS trace_bundles (
    bundle_digest TEXT PRIMARY KEY,
    archive_digest TEXT NOT NULL UNIQUE,
    archive_path TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    compatibility_level TEXT NOT NULL,
    validation_status TEXT NOT NULL,
    self_contained INTEGER NOT NULL,
    source_kind TEXT NOT NULL,
    source_uri TEXT,
    manifest_generation INTEGER,
    object_count INTEGER NOT NULL DEFAULT 0,
    byte_size INTEGER NOT NULL DEFAULT 0,
    imported_at TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS trace_bundle_members (
    bundle_digest TEXT NOT NULL REFERENCES trace_bundles(bundle_digest) ON DELETE CASCADE,
    trace_row_id TEXT NOT NULL REFERENCES traces(id) ON DELETE CASCADE,
    trace_digest TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    capture_id TEXT,
    binding_digest TEXT,
    sealed_path TEXT,
    PRIMARY KEY (bundle_digest, trace_digest)
);

CREATE INDEX IF NOT EXISTS trace_bundle_members_trace_digest
ON trace_bundle_members(trace_digest);

CREATE TABLE IF NOT EXISTS trace_assets (
    bundle_digest TEXT NOT NULL REFERENCES trace_bundles(bundle_digest) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    kind TEXT NOT NULL,
    role TEXT,
    bytes_digest TEXT NOT NULL,
    semantic_digest TEXT,
    media_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    availability TEXT NOT NULL,
    PRIMARY KEY (bundle_digest, relative_path)
);

CREATE TABLE IF NOT EXISTS trace_index (
    trace_digest TEXT PRIMARY KEY,
    projector_version TEXT NOT NULL,
    trace_kind TEXT,
    producer TEXT,
    model TEXT,
    provider TEXT,
    harness TEXT,
    benchmark TEXT,
    task_id TEXT,
    seed INTEGER,
    terminal_reason TEXT,
    lifecycle_status TEXT,
    capture_status TEXT,
    reward REAL,
    cost_usd REAL,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    span_count INTEGER NOT NULL DEFAULT 0,
    event_count INTEGER NOT NULL DEFAULT 0,
    tool_call_count INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    started_at TEXT,
    ended_at TEXT,
    duration_ms INTEGER,
    has_media INTEGER NOT NULL DEFAULT 0,
    has_evidence INTEGER NOT NULL DEFAULT 0,
    search_text TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS trace_index_model ON trace_index(model);
CREATE INDEX IF NOT EXISTS trace_index_benchmark_task ON trace_index(benchmark, task_id);
CREATE INDEX IF NOT EXISTS trace_index_reward ON trace_index(reward);

CREATE TABLE IF NOT EXISTS trace_tags (
    trace_digest TEXT NOT NULL,
    namespace TEXT NOT NULL,
    value TEXT NOT NULL,
    source_digest TEXT,
    PRIMARY KEY (trace_digest, namespace, value)
);

CREATE TABLE IF NOT EXISTS trace_projection_cache (
    trace_digest TEXT NOT NULL,
    projection_kind TEXT NOT NULL,
    projection_schema TEXT NOT NULL,
    projector_version TEXT NOT NULL,
    source_digest TEXT NOT NULL,
    payload_digest TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (trace_digest, projection_kind, projector_version)
);

CREATE TABLE IF NOT EXISTS trace_annotations (
    id TEXT PRIMARY KEY,
    trace_digest TEXT NOT NULL,
    selector_json TEXT NOT NULL,
    kind TEXT NOT NULL,
    body_json TEXT NOT NULL,
    author_json TEXT NOT NULL,
    supersedes_id TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS trace_annotations_trace_created
ON trace_annotations(trace_digest, created_at);
"#;

const MIGRATION_4: &str = r#"
CREATE TABLE IF NOT EXISTS optimizer_runs (
    id TEXT PRIMARY KEY,
    algorithm_id TEXT NOT NULL,
    algorithm_version TEXT,
    status TEXT NOT NULL,
    source TEXT NOT NULL,
    objective TEXT,
    project_ref TEXT,
    session_ref TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    cursor_seq INTEGER NOT NULL DEFAULT 0,
    capabilities_json TEXT NOT NULL DEFAULT '{}',
    bindings_json TEXT NOT NULL DEFAULT '[]',
    input_refs_json TEXT NOT NULL DEFAULT '[]',
    output_refs_json TEXT NOT NULL DEFAULT '[]',
    visual_refs_json TEXT NOT NULL DEFAULT '[]',
    summary_json TEXT NOT NULL DEFAULT '{}',
    usage_json TEXT NOT NULL DEFAULT '{}',
    error_json TEXT,
    payload_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS optimizer_runs_status ON optimizer_runs(status);
CREATE INDEX IF NOT EXISTS optimizer_runs_algorithm ON optimizer_runs(algorithm_id);
CREATE INDEX IF NOT EXISTS optimizer_runs_updated ON optimizer_runs(updated_at);

CREATE TABLE IF NOT EXISTS optimizer_event_cursors (
    optimizer_run_id TEXT PRIMARY KEY REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    cursor_seq INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimizer_events (
    event_id TEXT PRIMARY KEY,
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    sequence_number INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    algorithm_id TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (optimizer_run_id, sequence_number)
);

CREATE INDEX IF NOT EXISTS optimizer_events_run_seq
ON optimizer_events(optimizer_run_id, sequence_number);

CREATE TABLE IF NOT EXISTS optimizer_relationships (
    from_kind TEXT NOT NULL,
    from_id TEXT NOT NULL,
    edge TEXT NOT NULL,
    to_kind TEXT NOT NULL,
    to_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    PRIMARY KEY (from_kind, from_id, edge, to_kind, to_id)
);

CREATE INDEX IF NOT EXISTS optimizer_relationships_to
ON optimizer_relationships(to_kind, to_id);

CREATE TABLE IF NOT EXISTS optimizer_cached_slices (
    optimizer_run_id TEXT NOT NULL REFERENCES optimizer_runs(id) ON DELETE CASCADE,
    slice_id TEXT NOT NULL,
    cursor_seq INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (optimizer_run_id, slice_id)
);
"#;

const MIGRATION_5: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_workspace_scopes (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    workspace TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    bound_revision INTEGER NOT NULL DEFAULT 0,
    binding_status TEXT NOT NULL DEFAULT 'pending',
    binding_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace_attachments (
    session_id TEXT NOT NULL REFERENCES conversation_workspace_scopes(session_id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    access TEXT NOT NULL CHECK(access IN ('read_only', 'read_write')),
    source TEXT NOT NULL CHECK(source IN ('user_picker', 'agent_request', 'migrated_default')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (session_id, path)
);

CREATE TABLE IF NOT EXISTS workspace_grant_requests (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    access TEXT NOT NULL CHECK(access IN ('read_only', 'read_write')),
    reason TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'approved', 'denied')),
    created_at TEXT NOT NULL,
    resolved_at TEXT
);
CREATE INDEX IF NOT EXISTS workspace_grants_session_status
ON workspace_grant_requests(session_id, status, created_at);
"#;

const MIGRATION_6: &str = r#"
ALTER TABLE workspace_attachments RENAME TO workspace_attachments_v5;
CREATE TABLE workspace_attachments (
    session_id TEXT NOT NULL REFERENCES conversation_workspace_scopes(session_id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    access TEXT NOT NULL CHECK(access IN ('read_only', 'read_write')),
    source TEXT NOT NULL CHECK(source IN ('user_picker', 'recent_folder', 'agent_request', 'migrated_default')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (session_id, path)
);
INSERT INTO workspace_attachments(session_id,path,access,source,created_at)
SELECT session_id,path,access,source,created_at FROM workspace_attachments_v5;
DROP TABLE workspace_attachments_v5;
"#;

const MIGRATION_7: &str = r#"
CREATE TABLE IF NOT EXISTS model_performance_samples (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_revision TEXT,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    request_id TEXT NOT NULL,
    measurement_kind TEXT NOT NULL CHECK(measurement_kind IN ('decode', 'observed_stream', 'end_to_end', 'provider_reported')),
    status TEXT NOT NULL CHECK(status IN ('completed', 'failed', 'interrupted')),
    started_at_ms INTEGER NOT NULL,
    first_output_at_ms INTEGER,
    last_output_at_ms INTEGER,
    completed_at_ms INTEGER NOT NULL,
    input_tokens INTEGER,
    cached_input_tokens INTEGER,
    reasoning_tokens INTEGER,
    output_tokens INTEGER,
    ttft_ms REAL,
    observed_output_tps REAL,
    end_to_end_output_tps REAL,
    source TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(provider, request_id)
);

CREATE INDEX IF NOT EXISTS model_performance_model_created
ON model_performance_samples(provider, model_id, measurement_kind, created_at DESC);

CREATE INDEX IF NOT EXISTS model_performance_completed
ON model_performance_samples(status, completed_at_ms DESC);
"#;

/// One authoritative accounting record per provider request. Supersedes
/// `model_performance_samples` (all rows are imported, then the old table is
/// dropped) and takes over as the source for both throughput summaries and
/// the usage dashboard. The legacy `usage_ledger` table is left in place
/// until migration 11 folds its rows into this ledger and drops it.
///
/// `total_tokens` is a generated column so `input + output` can never drift
/// from its parts, and it stays NULL — not zero — while neither side has been
/// reported. Unlike the performance table this one has no retention cap:
/// billing history must never be silently trimmed.
const MIGRATION_8: &str = r#"
CREATE TABLE IF NOT EXISTS usage_records (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_revision TEXT,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    request_id TEXT NOT NULL,
    measurement_kind TEXT NOT NULL CHECK(measurement_kind IN ('decode', 'observed_stream', 'end_to_end', 'provider_reported')),
    status TEXT NOT NULL CHECK(status IN ('completed', 'failed', 'interrupted')),
    started_at_ms INTEGER NOT NULL,
    first_output_at_ms INTEGER,
    last_output_at_ms INTEGER,
    completed_at_ms INTEGER NOT NULL,
    input_tokens INTEGER,
    cached_input_tokens INTEGER,
    cache_write_tokens INTEGER,
    reasoning_tokens INTEGER,
    output_tokens INTEGER,
    total_tokens INTEGER GENERATED ALWAYS AS (
        CASE WHEN input_tokens IS NULL AND output_tokens IS NULL THEN NULL
             ELSE COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) END
    ) STORED,
    ttft_ms REAL,
    observed_output_tps REAL,
    end_to_end_output_tps REAL,
    billed_cost_usd REAL,
    estimated_cost_usd REAL,
    cost_source TEXT NOT NULL DEFAULT 'none' CHECK(cost_source IN ('provider_reported', 'synth_cloud', 'tariff_estimate', 'none')),
    source TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(provider, request_id)
);

-- OR IGNORE: recovery for databases wedged by a pre-transactional build that
-- crashed after this copy committed but before the version stamp did —
-- replaying the copy must skip rows that already arrived, not fail the open.
INSERT OR IGNORE INTO usage_records (
    id, provider, model_id, model_revision, session_id, run_id, request_id,
    measurement_kind, status, started_at_ms, first_output_at_ms, last_output_at_ms,
    completed_at_ms, input_tokens, cached_input_tokens, reasoning_tokens,
    output_tokens, ttft_ms, observed_output_tps, end_to_end_output_tps,
    source, created_at
)
SELECT
    id, provider, model_id, model_revision, session_id, run_id, request_id,
    measurement_kind, status, started_at_ms, first_output_at_ms, last_output_at_ms,
    completed_at_ms, input_tokens, cached_input_tokens, reasoning_tokens,
    output_tokens, ttft_ms, observed_output_tps, end_to_end_output_tps,
    source, created_at
FROM model_performance_samples;

DROP TABLE model_performance_samples;

CREATE INDEX IF NOT EXISTS usage_records_model_created
ON usage_records(provider, model_id, measurement_kind, created_at DESC);

CREATE INDEX IF NOT EXISTS usage_records_completed
ON usage_records(status, completed_at_ms DESC);

CREATE INDEX IF NOT EXISTS usage_records_window
ON usage_records(completed_at_ms DESC);
"#;

/// First-class SessionKind column. Authority moves off `target_json.kind`
/// string checks (`metadata_bags_are_not_authority`). Backfill treats
/// `"intern"` as Intern and everything else (including historical `"local"`
/// Codex/Laguna bags) as Codex.
const MIGRATION_9: &str = r#"
ALTER TABLE sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'codex';

UPDATE sessions
SET kind = 'intern'
WHERE json_extract(target_json, '$.kind') = 'intern';

CREATE INDEX IF NOT EXISTS sessions_kind ON sessions(kind);
"#;

/// Typed RuntimeTarget index + normalize legacy remote+synth-cloud bags to cloud.
const MIGRATION_10: &str = r#"
ALTER TABLE sessions ADD COLUMN runtime_target_kind TEXT NOT NULL DEFAULT 'local';

UPDATE sessions SET runtime_target_kind = CASE
    WHEN json_extract(target_json, '$.kind') = 'intern' THEN 'intern'
    WHEN json_extract(target_json, '$.kind') = 'cloud' THEN 'cloud'
    WHEN json_extract(target_json, '$.kind') = 'remote'
         AND json_extract(target_json, '$.provider') = 'synth-cloud' THEN 'cloud'
    WHEN json_extract(target_json, '$.kind') = 'remote' THEN 'remote'
    WHEN json_extract(target_json, '$.kind') = 'local' THEN 'local'
    WHEN json_extract(target_json, '$.kind') = 'codex' THEN 'local'
    ELSE 'local'
END;

-- Canonical CloudRuntime wire: kind=cloud (drop provider=synth-cloud).
UPDATE sessions
SET target_json = json_object(
    'kind', 'cloud',
    'model', COALESCE(json_extract(target_json, '$.model'), ''),
    'adapter', json_extract(target_json, '$.adapter')
)
WHERE runtime_target_kind = 'cloud'
  AND (
    json_extract(target_json, '$.kind') = 'remote'
    OR json_extract(target_json, '$.provider') = 'synth-cloud'
  );

CREATE INDEX IF NOT EXISTS sessions_runtime_target_kind
ON sessions(runtime_target_kind);
"#;

/// Shared copy used by migration 11 and by every later open. Rows are deleted
/// only after their stable legacy request id exists in `usage_records`; an
/// unexpected primary-key collision therefore retains the source row rather
/// than silently losing accounting data.
const FOLD_LEGACY_USAGE_ROWS: &str = r#"
INSERT OR IGNORE INTO usage_records (
    id, provider, model_id, session_id, run_id, request_id,
    measurement_kind, status, started_at_ms, completed_at_ms,
    input_tokens, output_tokens,
    billed_cost_usd, estimated_cost_usd, cost_source,
    source, created_at
)
SELECT
    id,
    provider,
    model,
    CASE
        WHEN session_id IS NOT NULL
             AND EXISTS (SELECT 1 FROM sessions s WHERE s.id = usage_ledger.session_id)
            THEN session_id
        ELSE NULL
    END,
    CASE
        WHEN run_id IS NOT NULL
             AND EXISTS (SELECT 1 FROM runs r WHERE r.id = usage_ledger.run_id)
            THEN run_id
        ELSE NULL
    END,
    'legacy-ledger:' || id,
    'provider_reported',
    'completed',
    COALESCE(CAST(strftime('%s', substr(created_at, 1, 19)) AS INTEGER), 0) * 1000,
    COALESCE(CAST(strftime('%s', substr(created_at, 1, 19)) AS INTEGER), 0) * 1000,
    prompt_tokens,
    completion_tokens,
    cost_usd,
    NULL,
    CASE WHEN cost_usd IS NOT NULL THEN 'provider_reported' ELSE 'none' END,
    'legacy_usage_ledger',
    created_at
FROM usage_ledger;

-- A primary-key collision with an unrelated usage record must not discard the
-- rollback row. Leave it buffered unless its legacy request identity landed.
DELETE FROM usage_ledger
WHERE EXISTS (
    SELECT 1 FROM usage_records
    WHERE request_id = 'legacy-ledger:' || usage_ledger.id
      AND provider = usage_ledger.provider
      AND source = 'legacy_usage_ledger'
);
"#;

/// Fold the legacy ledger into `usage_records`, then retain its empty schema as
/// a rollback write buffer. The previous production binary reads and writes
/// this table; dropping it makes rollback fail with `no such table`.
const MIGRATION_11: &str = FOLD_LEGACY_USAGE_ROWS;

/// Additive repair for databases already stamped at the original migration 11,
/// which dropped `usage_ledger`. Fresh upgrades retain the migration-1 table;
/// already-upgraded QA databases recreate the exact legacy schema here.
const MIGRATION_12: &str = r#"
CREATE TABLE IF NOT EXISTS usage_ledger (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL,
    created_at TEXT NOT NULL
);
"#;

const MIGRATION_13: &str = r#"
CREATE TABLE IF NOT EXISTS visual_renditions (
    visual_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    format TEXT NOT NULL,
    theme TEXT NOT NULL,
    size_class TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    media_type TEXT NOT NULL,
    renderer_version TEXT NOT NULL,
    width_px INTEGER,
    height_px INTEGER,
    created_at TEXT NOT NULL,
    PRIMARY KEY (visual_id, revision, format, theme, size_class),
    FOREIGN KEY (visual_id, revision)
        REFERENCES visual_revisions(visual_id, revision)
);
"#;

const MIGRATION_14: &str = r#"
CREATE TABLE IF NOT EXISTS visual_annotations (
    id TEXT PRIMARY KEY,
    visual_id TEXT NOT NULL REFERENCES visuals(id) ON DELETE CASCADE,
    visual_revision INTEGER NOT NULL,
    source_digest TEXT,
    selector_json TEXT NOT NULL,
    kind TEXT NOT NULL,
    body TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    author_id TEXT NOT NULL,
    supersedes_id TEXT REFERENCES visual_annotations(id),
    tombstoned INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (visual_id, visual_revision)
        REFERENCES visual_revisions(visual_id, revision)
);

CREATE INDEX IF NOT EXISTS visual_annotations_visual_revision
ON visual_annotations(visual_id, visual_revision, created_at);

CREATE TABLE IF NOT EXISTS visual_seals (
    receipt_digest TEXT PRIMARY KEY,
    visual_id TEXT NOT NULL REFERENCES visuals(id) ON DELETE CASCADE,
    visual_revision INTEGER NOT NULL,
    artifact_id TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    compiler_name TEXT NOT NULL,
    compiler_version TEXT NOT NULL,
    runtime_digest TEXT NOT NULL,
    index_digest TEXT NOT NULL,
    data_digest TEXT NOT NULL,
    receipt_size_bytes INTEGER NOT NULL,
    total_size_bytes INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (visual_id, visual_revision)
        REFERENCES visual_revisions(visual_id, revision)
);

CREATE INDEX IF NOT EXISTS visual_seals_visual_revision
ON visual_seals(visual_id, visual_revision, created_at);

CREATE TABLE IF NOT EXISTS visual_uploads (
    receipt_digest TEXT PRIMARY KEY REFERENCES visual_seals(receipt_digest) ON DELETE CASCADE,
    collection_id TEXT,
    publication_id TEXT,
    publication_revision INTEGER,
    prepare_expires_at TEXT,
    completed_members_json TEXT NOT NULL DEFAULT '[]',
    state TEXT NOT NULL,
    committed_url TEXT,
    error TEXT,
    updated_at TEXT NOT NULL,
    CHECK (state IN ('prepared','uploading','finalizing','committed','failed')),
    CHECK ((state = 'committed' AND committed_url IS NOT NULL) OR (state != 'committed' AND committed_url IS NULL))
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// A database stamped `version` with migrations 1..=version applied, as a
    /// real installation of that era would have shipped it.
    fn seed_at_version(version: usize) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        for (index, migration) in MIGRATIONS.iter().take(version).enumerate() {
            conn.execute_batch(migration).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, datetime('now'))",
                [index as i64 + 1],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn upgrades_v1_database_without_losing_runs() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions(id, title, target_json, status, created_at, updated_at)
             VALUES ('session-1', 'Session', '{}', 'ready', 'before', 'before')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO runs(id, session_id, mode, status, created_at)
             VALUES ('run-1', 'session-1', 'test', 'completed', 'before')",
            [],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), 14);
        let updated_at: String = conn
            .query_row(
                "SELECT updated_at FROM runs WHERE id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(updated_at, "before");
        let receipts: i64 = conn
            .query_row("SELECT COUNT(*) FROM command_receipts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(receipts, 0);
        let trace_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('trace_imports','trace_bundles','trace_bundle_members','trace_assets','trace_index','trace_tags','trace_projection_cache','trace_annotations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(trace_tables, 8);
        let accounting_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_records'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accounting_tables, 1);
        // The performance table was folded into usage_records by migration 8;
        // Migration 11 folds the legacy rows; migration 12 guarantees the
        // empty rollback staging schema for already-upgraded QA databases.
        let performance_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'model_performance_samples'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(performance_tables, 0);
        let ledger_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_ledger'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            ledger_tables, 1,
            "rollback staging schema remains available"
        );
    }

    /// A v7 database imports performance samples into `usage_records`, then
    /// migration 11 folds legacy `usage_ledger` rows into the same ledger.
    #[test]
    fn migration_8_and_11_fold_ledger_and_import_performance_samples() {
        let conn = seed_at_version(7);
        conn.execute(
            "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
             VALUES('ledger-1','openrouter','luna',10,20,30,0.5,'2026-08-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO model_performance_samples(id,provider,model_id,request_id,measurement_kind,status,started_at_ms,completed_at_ms,input_tokens,cached_input_tokens,reasoning_tokens,output_tokens,ttft_ms,observed_output_tps,source,created_at)
             VALUES('perf-1','openrouter','openai/gpt-5.6-luna','req-1','observed_stream','completed',1000,2000,100,40,5,50,120.0,25.0,'codex_app_server','2026-08-01T00:00:01Z')",
            [],
        )
        .unwrap();

        assert_eq!(apply_migrations(&conn).unwrap(), 14);

        let ledger_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_ledger'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger_tables, 1);
        let staged: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_ledger", [], |row| row.get(0))
            .unwrap();
        assert_eq!(staged, 0, "folded rows are cleared from rollback staging");
        let (billed, cost_source, source): (f64, String, String) = conn
            .query_row(
                "SELECT billed_cost_usd, cost_source, source
                 FROM usage_records WHERE request_id='legacy-ledger:ledger-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert!((billed - 0.5).abs() < 1e-9);
        assert_eq!(cost_source, "provider_reported");
        assert_eq!(source, "legacy_usage_ledger");
        let (input, cached, total, cost_source): (i64, i64, i64, String) = conn
            .query_row(
                "SELECT input_tokens, cached_input_tokens, total_tokens, cost_source
                 FROM usage_records WHERE provider='openrouter' AND request_id='req-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!((input, cached, total), (100, 40, 150));
        assert_eq!(cost_source, "none");
    }

    /// Candidate builds already applied the original migration 11, which
    /// folded rows and dropped the table. Migration 12 repairs that database
    /// for rollback. This query is copied from the pre-Wave-7 account summary:
    /// the old binary sums both ledgers, so compatibility must not mirror rows
    /// into both places or historical usage would double-count.
    #[test]
    fn migration_12_repairs_original_v11_and_folds_rollback_writes_once() {
        let conn = seed_at_version(10);
        conn.execute(
            "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
             VALUES('before-v11','openrouter','luna',10,20,30,0.5,'2026-08-01T00:00:00Z')",
            [],
        )
        .unwrap();

        // Exact original candidate migration-11 shape: fold, drop, stamp.
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE;\n{FOLD_LEGACY_USAGE_ROWS}\nDROP TABLE usage_ledger;\nINSERT INTO schema_migrations(version, applied_at) VALUES (11, datetime('now'));\nCOMMIT;"
        ))
        .unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 11);

        assert_eq!(apply_migrations(&conn).unwrap(), 14);
        let old_binary_sum = |conn: &Connection| -> f64 {
            conn.query_row(
                "SELECT
                    (SELECT COALESCE(SUM(COALESCE(billed_cost_usd, estimated_cost_usd)), 0)
                     FROM usage_records
                     WHERE COALESCE(billed_cost_usd, estimated_cost_usd) IS NOT NULL)
                    +
                    (SELECT COALESCE(SUM(cost_usd), 0)
                     FROM usage_ledger
                     WHERE cost_usd IS NOT NULL)",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        let old_inventory_count = |conn: &Connection| -> i64 {
            conn.query_row(
                "SELECT (SELECT COUNT(*) FROM usage_records) + (SELECT COUNT(*) FROM usage_ledger)",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert!((old_binary_sum(&conn) - 0.5).abs() < 1e-9);
        assert_eq!(old_inventory_count(&conn), 1);

        // A rolled-back v10 binary can write the legacy schema without error.
        conn.execute(
            "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
             VALUES('during-rollback','synth','laguna-s',4,6,10,0.25,'2026-08-02T00:00:00Z')",
            [],
        )
        .unwrap();
        assert!((old_binary_sum(&conn) - 0.75).abs() < 1e-9);
        assert_eq!(old_inventory_count(&conn), 2);

        // Relaunching v12 folds and clears staging. The union remains exactly
        // the same, proving neither data loss nor a transient double charge.
        assert_eq!(apply_migrations(&conn).unwrap(), 14);
        assert!((old_binary_sum(&conn) - 0.75).abs() < 1e-9);
        assert_eq!(old_inventory_count(&conn), 2);
        let old_inventory_ids: Vec<String> = {
            let mut statement = conn
                .prepare(
                    "SELECT id FROM (
                        SELECT id, created_at FROM usage_records
                        UNION ALL
                        SELECT id, created_at FROM usage_ledger
                     ) ORDER BY created_at DESC, id",
                )
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(
            old_inventory_ids,
            vec!["during-rollback".to_string(), "before-v11".to_string()]
        );
        let (staged, folded): (i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM usage_ledger),
                    (SELECT COUNT(*) FROM usage_records WHERE request_id='legacy-ledger:during-rollback')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((staged, folded), (0, 1));
    }

    /// A realistic v7 history — a large sample backlog and samples with
    /// unreported token counts — comes across whole: every row imported (no
    /// retention trim), unreported counters stay NULL (no silent zeroes), and
    /// generated totals equal input + output where both exist.
    #[test]
    fn migration_8_imports_every_sample_and_never_zeroes_unreported_tokens() {
        let conn = seed_at_version(7);
        conn.execute(
            "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
             VALUES('ledger-1','openrouter','luna',10,20,30,0.5,'2026-08-01T00:00:00Z')",
            [],
        )
        .unwrap();
        // A sample whose provider never reported token counts at all.
        conn.execute(
            "INSERT INTO model_performance_samples(id,provider,model_id,request_id,measurement_kind,status,started_at_ms,completed_at_ms,source,created_at)
             VALUES('perf-null','openrouter','openai/gpt-5.6-luna','req-null','end_to_end','failed',1000,1500,'codex_app_server','2026-08-01T00:00:01Z')",
            [],
        )
        .unwrap();
        for index in 0..1_200 {
            conn.execute(
                "INSERT INTO model_performance_samples(id,provider,model_id,request_id,measurement_kind,status,started_at_ms,completed_at_ms,input_tokens,output_tokens,source,created_at)
                 VALUES(?1,'openrouter','openai/gpt-5.6-luna',?2,'observed_stream','completed',1000,2000,100,50,'codex_app_server','2026-08-01T00:00:02Z')",
                rusqlite::params![format!("perf-{index}"), format!("req-{index}")],
            )
            .unwrap();
        }

        assert_eq!(apply_migrations(&conn).unwrap(), 14);

        let imported: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            imported, 1_202,
            "history must never be silently trimmed (perf samples + folded ledger)"
        );
        let (null_total, billed, estimated, cost_source): (
            Option<i64>,
            Option<f64>,
            Option<f64>,
            String,
        ) = conn
            .query_row(
                "SELECT total_tokens, billed_cost_usd, estimated_cost_usd, cost_source
                 FROM usage_records WHERE request_id='req-null'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(null_total, None, "unreported tokens must not become zero");
        assert_eq!((billed, estimated), (None, None));
        assert_eq!(cost_source, "none");
        let (totals_ok, ledger_tables, folded): (i64, i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM usage_records WHERE request_id LIKE 'req-%' AND request_id != 'req-null' AND total_tokens = 150),
                        (SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='usage_ledger'),
                        (SELECT COUNT(*) FROM usage_records WHERE request_id='legacy-ledger:ledger-1')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(totals_ok, 1_200);
        assert_eq!(ledger_tables, 1, "rollback staging schema remains");
        assert_eq!(folded, 1, "legacy ledger rows land in usage_records");
    }

    /// Recovery from the pre-transactional failure mode: a crash landed after
    /// migration 8's data copy but before its version stamp, so the app
    /// relaunches at version 7 with `usage_records` already populated.
    /// Replaying the migration must recover — not fail `Database::open`
    /// forever on the copy's primary key.
    #[test]
    fn a_partially_applied_migration_8_recovers_instead_of_wedging_open() {
        let conn = seed_at_version(7);
        conn.execute(
            "INSERT INTO model_performance_samples(id,provider,model_id,request_id,measurement_kind,status,started_at_ms,completed_at_ms,input_tokens,output_tokens,source,created_at)
             VALUES('perf-1','openrouter','openai/gpt-5.6-luna','req-1','observed_stream','completed',1000,2000,100,50,'codex_app_server','2026-08-01T00:00:01Z')",
            [],
        )
        .unwrap();
        // Everything up to (but excluding) the DROP: the create and the copy
        // committed, the version stamp did not.
        let partial = MIGRATION_8
            .split("DROP TABLE")
            .next()
            .expect("migration 8 drops the samples table");
        conn.execute_batch(partial).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 7);

        assert_eq!(apply_migrations(&conn).unwrap(), 14);
        let (copies, leftovers): (i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM usage_records WHERE request_id='req-1'),
                        (SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_performance_samples')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(copies, 1, "the replayed copy must not duplicate rows");
        assert_eq!(leftovers, 0);
    }

    /// A migration that fails mid-flight must leave no half-applied state
    /// behind: its statements and its version stamp commit atomically.
    #[test]
    fn session_kind_backfills_from_target_json() {
        let conn = seed_at_version(8);
        conn.execute(
            r#"INSERT INTO sessions(id, title, target_json, status, created_at, updated_at)
               VALUES
                 ('codex-1', 'Codex', '{"kind":"codex"}', 'ready', 'now', 'now'),
                 ('local-1', 'Local', '{"kind":"local"}', 'ready', 'now', 'now'),
                 ('intern-1', 'Intern', '{"kind":"intern","mode":"sync"}', 'ready', 'now', 'now')"#,
            [],
        )
        .unwrap();
        assert_eq!(apply_migrations(&conn).unwrap(), 14);
        let kinds: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, kind FROM sessions ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            kinds,
            vec![
                ("codex-1".into(), "codex".into()),
                ("intern-1".into(), "intern".into()),
                ("local-1".into(), "codex".into()),
            ]
        );
    }

    #[test]
    fn a_failing_migration_leaves_neither_data_nor_version_behind() {
        let conn = seed_at_version(7);
        let poison = "CREATE TABLE wp4_atomicity_probe (id TEXT PRIMARY KEY);\nTHIS IS NOT SQL;";
        let error = apply_one(&conn, 8, poison).expect_err("the batch must fail");
        assert!(error.to_string().contains("apply schema migration 8"));
        let (probe, version): (i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM sqlite_master WHERE name='wp4_atomicity_probe'),
                        (SELECT COALESCE(MAX(version),0) FROM schema_migrations)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(probe, 0, "the failed migration's table must roll back");
        assert_eq!(version, 7);
        // The connection stays usable and the real migration still applies.
        assert_eq!(apply_migrations(&conn).unwrap(), 14);
    }

    #[test]
    fn migration_10_indexes_runtime_target_kind_and_normalizes_cloud() {
        let conn = seed_at_version(9);
        conn.execute(
            "INSERT INTO sessions(id, title, target_json, status, created_at, updated_at)
             VALUES ('cloud-1', 'Cloud', ?1, 'ready', 'now', 'now')",
            [r#"{"kind":"remote","provider":"synth-cloud","model":"openrouter/poolside/laguna-s-2.1","adapter":null}"#],
        )
        .unwrap();
        assert_eq!(apply_migrations(&conn).unwrap(), 14);
        let (kind, target): (String, String) = conn
            .query_row(
                "SELECT runtime_target_kind, target_json FROM sessions WHERE id='cloud-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "cloud");
        assert!(target.contains(r#""kind":"cloud""#) || target.contains(r#""kind": "cloud""#));
        let indexed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='sessions_runtime_target_kind'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 1);
    }

    #[test]
    fn migration_14_never_allows_a_partial_upload_permalink() {
        let conn = seed_at_version(13);
        assert_eq!(apply_migrations(&conn).unwrap(), 14);
        conn.execute(
            "INSERT INTO visuals(id,current_revision,title,template_id,status,renderer_kind,bindings_json,metadata_json,created_at,updated_at)
             VALUES ('vis-1',1,'Visual','template.v1','saved','template','{}','{}','now','now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO visual_revisions(visual_id,revision,template_id,renderer_kind,bindings_json,created_at)
             VALUES ('vis-1',1,'template.v1','template','{}','now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO visual_seals(receipt_digest,visual_id,visual_revision,artifact_id,schema_version,compiler_name,compiler_version,runtime_digest,index_digest,data_digest,receipt_size_bytes,total_size_bytes,created_at)
             VALUES (?1,'vis-1',1,'visual:vis-1','synth.artifact-bundle.v1','workshop','0.3',?2,?3,?4,10,30,'now')",
            ["a".repeat(64), "b".repeat(64), "c".repeat(64), "d".repeat(64)],
        )
        .unwrap();
        let partial_with_url = conn.execute(
            "INSERT INTO visual_uploads(receipt_digest,state,committed_url,updated_at)
             VALUES (?1,'uploading','https://should-not-exist','now')",
            ["a".repeat(64)],
        );
        assert!(partial_with_url.is_err());
        let committed_without_url = conn.execute(
            "INSERT INTO visual_uploads(receipt_digest,state,updated_at)
             VALUES (?1,'committed','now')",
            ["a".repeat(64)],
        );
        assert!(committed_without_url.is_err());
        conn.execute(
            "INSERT INTO visual_uploads(receipt_digest,state,committed_url,updated_at)
             VALUES (?1,'committed','https://private.example/p','now')",
            ["a".repeat(64)],
        )
        .unwrap();
    }
}
