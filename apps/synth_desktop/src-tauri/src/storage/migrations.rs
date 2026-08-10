use anyhow::{Context, Result};
use rusqlite::Connection;

/// Ordered embedded migrations. Version N applies only after 1..=N-1.
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

    if current < 1 {
        conn.execute_batch(MIGRATION_1)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'))",
            [],
        )?;
    }

    if current < 2 {
        conn.execute_batch(MIGRATION_2)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (2, datetime('now'))",
            [],
        )?;
    }

    if current < 3 {
        conn.execute_batch(MIGRATION_3)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (3, datetime('now'))",
            [],
        )?;
    }

    if current < 4 {
        conn.execute_batch(MIGRATION_4)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (4, datetime('now'))",
            [],
        )?;
    }

    if current < 5 {
        conn.execute_batch(MIGRATION_5)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (5, datetime('now'))",
            [],
        )?;
    }

    if current < 6 {
        conn.execute_batch(MIGRATION_6)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (6, datetime('now'))",
            [],
        )?;
    }

    if current < 7 {
        conn.execute_batch(MIGRATION_7)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (7, datetime('now'))",
            [],
        )?;
    }

    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    Ok(version)
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

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_eq!(apply_migrations(&conn).unwrap(), 7);
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
        let performance_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'model_performance_samples'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(performance_tables, 1);
    }
}
